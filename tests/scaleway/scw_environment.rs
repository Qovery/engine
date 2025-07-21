use crate::helpers;
use crate::helpers::common::Infrastructure;
use crate::helpers::environment::session_is_sticky;
use crate::helpers::scaleway::clean_environments;
use crate::helpers::scaleway::scw_infra_config;
use crate::helpers::utilities::{
    FuncTestsSecrets, context_for_resource, engine_run_test, get_pods, logger, metrics_registry,
};
use crate::helpers::utilities::{get_pvc, is_pod_restarted_env};
use ::function_name::named;
use bstr::ByteSlice;
use qovery_engine::infrastructure::models::cloud_provider::Kind;
use qovery_engine::io_models::application::{Port, Protocol, Storage};

use crate::helpers::kubernetes::TargetCluster;
use base64::Engine;
use base64::engine::general_purpose;
use qovery_engine::cmd::kubectl::kubectl_get_secret;
use qovery_engine::environment::models::scaleway::ScwZone;
use qovery_engine::infrastructure::models::container_registry::InteractWithRegistry;
use qovery_engine::io_models::annotations_group::{Annotation, AnnotationsGroup, AnnotationsGroupScope};
use qovery_engine::io_models::container::{Container, Registry};
use qovery_engine::io_models::context::CloneForTest;
use qovery_engine::io_models::job::{ContainerRegistries, Job, JobSchedule, JobSource, LifecycleType};
use qovery_engine::io_models::labels_group::{Label, LabelsGroup};
use qovery_engine::io_models::probe::{Probe, ProbeType};
use qovery_engine::io_models::router::{CustomDomain, Route, Router};
use qovery_engine::io_models::variable_utils::VariableInfo;
use qovery_engine::io_models::{Action, MountedFile, QoveryIdentifier};
use qovery_engine::utilities::to_short_id;
use reqwest::StatusCode;
use retry::delay::Fibonacci;
use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;
use std::thread::sleep;
use std::time::{Duration, Instant};
use tracing::{Level, span, warn};
use url::Url;
use uuid::Uuid;
// Note: All those tests relies on a test cluster running on Scaleway infrastructure.
// This cluster should be live in order to have those tests passing properly.

#[cfg(feature = "test-quarantine")]
#[named]
#[test]
fn scaleway_test_build_phase() {
    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry.clone());
        let environment = helpers::environment::working_minimal_environment(&context);

        let env_action = environment.clone();

        let (env, ret) = environment.build_environment(&env_action, &infra_ctx);
        assert!(ret.is_ok());

        // Check the image exist in the registry
        let img_exist = infra_ctx
            .container_registry()
            .image_exists(&env.applications[0].get_build().image);
        assert!(img_exist);

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn scaleway_kapsule_deploy_a_working_environment_with_no_router() {
    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID"),
        );
        let region = ScwZone::from_str(
            secrets
                .SCALEWAY_TEST_CLUSTER_REGION
                .as_ref()
                .expect("SCALEWAY_TEST_CLUSTER_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown SCW region");
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
        );
        let environment = helpers::environment::working_minimal_environment(&context);

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let env_action = environment.clone();
        let env_action_for_delete = environment_for_delete.clone();

        let result = environment.deploy_environment(&env_action, &infra_ctx);
        assert!(result.is_ok());

        let result = environment_for_delete.delete_environment(&env_action_for_delete, &infra_ctx_for_delete);
        assert!(result.is_ok());

        if let Err(e) = clean_environments(&context, vec![environment], region) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
#[ignore = "This test does not work on CI yet"]
fn scaleway_kapsule_deploy_a_working_environment_with_shared_registry() {
    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };

        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
        );

        let repo_id = Uuid::new_v4().to_string();
        let container = helpers::git_server::init_git_server_testcontainer(repo_id.clone());

        let repo_url = format!(
            "http://{}:{}/{}.git",
            container.get_host().expect("git container has a host"),
            container
                .get_host_port_ipv4(80)
                .expect("git container has an exposed port"),
            repo_id
        );

        let environment =
            helpers::environment::working_minimal_environment_with_custom_git_url(&context, repo_url.as_str());
        let environment2 =
            helpers::environment::working_minimal_environment_with_custom_git_url(&context, repo_url.as_str());

        let env_to_deploy = environment.clone();
        let env_to_deploy2 = environment2.clone();

        let ret = environment.deploy_environment(&env_to_deploy, &infra_ctx);
        assert!(ret.is_ok());
        let ret = environment2.deploy_environment(&env_to_deploy2, &infra_ctx);
        assert!(ret.is_ok());
        // Check take both deployment used the same image
        let env = environment
            .to_environment_domain(
                infra_ctx.context(),
                infra_ctx.cloud_provider(),
                infra_ctx.container_registry(),
                infra_ctx.kubernetes(),
            )
            .expect("Environment should be valid");
        let env2 = environment2
            .to_environment_domain(
                infra_ctx.context(),
                infra_ctx.cloud_provider(),
                infra_ctx.container_registry(),
                infra_ctx.kubernetes(),
            )
            .expect("Environment should be valid");

        assert_eq!(
            env.applications[0].get_build().image.name,
            env2.applications[0].get_build().image.name
        );
        let img_exist = infra_ctx
            .container_registry()
            .image_exists(&env.applications[0].get_build().image);
        assert!(img_exist);

        let mut environment_to_delete = environment.clone();
        let mut environment_to_delete2 = environment2.clone();
        environment_to_delete.action = Action::Delete;
        environment_to_delete.applications[0].should_delete_shared_registry = false;
        environment_to_delete2.action = Action::Delete;
        environment_to_delete2.applications[0].should_delete_shared_registry = true;

        let ret = environment_to_delete.delete_environment(&environment_to_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());
        let img_exist = infra_ctx
            .container_registry()
            .image_exists(&env.applications[0].get_build().image);
        assert!(img_exist);
        let ret = environment_to_delete2.delete_environment(&environment_to_delete2, &infra_ctx_for_delete);
        assert!(ret.is_ok());
        let img_exist = infra_ctx
            .container_registry()
            .image_exists(&env.applications[0].get_build().image);
        assert!(!img_exist);
        let _created_repository_name_guard =
            scopeguard::guard(&env.applications[0].get_build().image.repository_name, |repository_name| {
                // make sure to delete the repository after test
                infra_ctx
                    .container_registry()
                    .delete_repository(repository_name.as_str())
                    .unwrap_or_else(|_| println!("Cannot delete test repository `{repository_name}` after test"));
            });
        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn scaleway_kapsule_deploy_a_not_working_environment_with_no_router() {
    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name,);
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID"),
        );
        let region = ScwZone::from_str(
            secrets
                .SCALEWAY_TEST_CLUSTER_REGION
                .as_ref()
                .expect("SCALEWAY_TEST_CLUSTER_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown SCW region");
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
        );

        let mut environment = helpers::environment::non_working_environment(&context);
        environment.routers = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let env_action = environment.clone();
        let env_action_for_delete = environment_for_delete.clone();

        let result = environment.deploy_environment(&env_action, &infra_ctx);
        assert!(result.is_err());

        let result = environment_for_delete.delete_environment(&env_action_for_delete, &infra_ctx_for_delete);
        assert!(matches!(result, Ok(_) | Err(_)));

        if let Err(e) = clean_environments(&context, vec![environment], region) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn scaleway_kapsule_deploy_a_working_environment_and_pause() {
    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID"),
        );
        let region = ScwZone::from_str(
            secrets
                .SCALEWAY_TEST_CLUSTER_REGION
                .as_ref()
                .expect("SCALEWAY_TEST_CLUSTER_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown SCW region");
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
        );
        let environment = helpers::environment::working_minimal_environment(&context);

        let env_action = environment.clone();
        let selector = &environment.applications[0].long_id;

        let result = environment.deploy_environment(&env_action, &infra_ctx);
        assert!(result.is_ok());

        let ret = get_pods(&infra_ctx, Kind::Scw, &environment, selector);
        assert!(ret.is_ok());
        assert!(!ret.unwrap().items.is_empty());

        let result = environment.pause_environment(&env_action, &infra_ctx_for_delete);
        assert!(result.is_ok());

        // Check that we have actually 0 pods running for this app
        let ret = get_pods(&infra_ctx, Kind::Scw, &environment, selector);
        assert!(ret.is_ok());
        assert!(ret.unwrap().items.is_empty());

        // Check we can resume the env
        let ctx_resume = context.clone_not_same_execution_id();
        let infra_ctx_resume =
            scw_infra_config(&target_cluster_scw_test, &ctx_resume, logger.clone(), metrics_registry.clone());
        let result = environment.deploy_environment(&env_action, &infra_ctx_resume);
        assert!(result.is_ok());

        let ret = get_pods(&infra_ctx, Kind::Scw, &environment, selector);
        assert!(ret.is_ok());
        assert!(!ret.unwrap().items.is_empty());

        // Cleanup
        let result = environment.delete_environment(&env_action, &infra_ctx_for_delete);
        assert!(result.is_ok());

        if let Err(e) = clean_environments(&context, vec![environment], region) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn scaleway_kapsule_deploy_a_working_environment_with_domain() {
    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test",);
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
        );
        let mut environment = helpers::environment::working_minimal_environment_with_router(
            &context,
            secrets
                .DEFAULT_TEST_DOMAIN
                .as_ref()
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

        let mut modified_environment = environment.clone();
        modified_environment.applications.clear();
        modified_environment.routers.clear();

        for (idx, router) in environment.routers.into_iter().enumerate() {
            // add custom domain
            let mut router = router.clone();
            let cd = CustomDomain {
                domain: format!("fake-custom-domain-{idx}.qovery.io"),
                target_domain: format!("validation-domain-{idx}"),
                generate_certificate: true,
                use_cdn: true, // disable custom domain check
            };

            router.custom_domains = vec![cd];
            modified_environment.routers.push(router);
        }

        for mut application in environment.applications.into_iter() {
            application.ports.push(Port {
                long_id: Uuid::new_v4(),
                port: 5050,
                is_default: false,
                name: "grpc".to_string(),
                publicly_accessible: true,
                protocol: Protocol::GRPC,
                service_name: None,
                namespace: None,
                additional_service: None,
            });
            modified_environment.applications.push(application);
        }

        environment = modified_environment;

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;

        let ea = environment.clone();
        let ea_delete = environment_delete.clone();

        let ret = environment.deploy_environment(&ea, &infra_ctx);
        assert!(ret.is_ok());

        let ret = environment_delete.delete_environment(&ea_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn scaleway_kapsule_deploy_a_working_environment_with_storage() {
    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name,);
        let _enter = span.enter();

        let secrets = FuncTestsSecrets::new();
        let logger = logger();
        let metrics_registry = metrics_registry();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID"),
        );
        let region = ScwZone::from_str(
            secrets
                .SCALEWAY_TEST_CLUSTER_REGION
                .as_ref()
                .expect("SCALEWAY_TEST_CLUSTER_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown SCW region");
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_deletion = context.clone_not_same_execution_id();
        let infra_ctx_for_deletion = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_deletion,
            logger.clone(),
            metrics_registry.clone(),
        );

        let mut environment = helpers::environment::working_minimal_environment(&context);

        let storage_size: u32 = 10;
        environment.applications = environment
            .applications
            .into_iter()
            .map(|mut app| {
                let id = Uuid::new_v4();
                app.storage = vec![Storage {
                    id: to_short_id(&id),
                    long_id: id,
                    name: "photos".to_string(),
                    storage_class: "scw-sbv-ssd-0".to_string(),
                    size_in_gib: storage_size,
                    mount_point: "/mnt/photos".to_string(),
                    snapshot_retention_in_days: 0,
                }];
                app
            })
            .collect::<Vec<qovery_engine::io_models::application::Application>>();

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;

        let env_action = environment.clone();
        let env_action_delete = environment_delete.clone();

        let result = environment.deploy_environment(&env_action, &infra_ctx);
        assert!(result.is_ok());

        match get_pvc(&infra_ctx, Kind::Scw, &environment) {
            Ok(pvc) => assert_eq!(
                pvc.items.expect("No items in pvc")[0].spec.resources.requests.storage,
                format!("{storage_size}Gi")
            ),
            Err(_) => panic!(),
        };

        let result = environment_delete.delete_environment(&env_action_delete, &infra_ctx_for_deletion);
        assert!(result.is_ok());

        if let Err(e) = clean_environments(&context, vec![environment], region) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn scaleway_kapsule_deploy_a_working_environment_with_mounted_files_as_volume() {
    // TODO(benjaminch): This test could be moved out of end to end tests as it doesn't require
    // any cloud provider to be performed (can run on local Kubernetes).

    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let secrets = FuncTestsSecrets::new();
        let logger = logger();
        let metrics_registry = metrics_registry();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_deletion = context.clone_not_same_execution_id();
        let infra_ctx_for_deletion = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_deletion,
            logger.clone(),
            metrics_registry.clone(),
        );

        let mounted_file_identifier = QoveryIdentifier::new_random();
        let mounted_file = MountedFile {
            id: mounted_file_identifier.short().to_string(),
            long_id: mounted_file_identifier.to_uuid(),
            mount_path: "/this-file-should-exist".to_string(),
            file_content_b64: general_purpose::STANDARD.encode("I exist !"),
        };

        let environment =
            helpers::environment::working_environment_with_application_and_stateful_crashing_if_file_doesnt_exist(
                &context,
                &mounted_file,
                "scw-sbv-ssd-0",
            );

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;

        let ea = environment.clone();
        let ea_delete = environment_delete.clone();

        let ret = environment.deploy_environment(&ea, &infra_ctx);
        assert!(ret.is_ok());

        // check if secret exists
        let service_id = QoveryIdentifier::new(
            environment
                .applications
                .first()
                .expect("there must be at least one application in environment")
                .long_id,
        )
        .short()
        .to_string();
        let config_maps = kubectl_get_secret(
            infra_ctx.mk_kube_client().expect("kube client is not set").client(),
            format!("metadata.name={}-{}", &mounted_file.id, service_id).as_str(),
        )
        .expect("unable to find secret for selector");
        assert!(!config_maps.is_empty());
        for cm in config_maps {
            assert_eq!(
                general_purpose::STANDARD
                    .decode(&mounted_file.file_content_b64)
                    .expect("mounted file content cannot be b64 decoded")
                    .to_str(),
                cm.data
                    .expect("data should be set")
                    .get("content")
                    .expect("content should exist")
                    .0
                    .to_str()
            );
        }

        let ret = environment_delete.delete_environment(&ea_delete, &infra_ctx_for_deletion);
        assert!(ret.is_ok());

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn deploy_a_working_environment_and_pause_it() {
    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
        );
        let environment = helpers::environment::working_minimal_environment(&context);

        let ea = environment.clone();
        let selector = &environment.applications[0].long_id;

        let result = environment.deploy_environment(&ea, &infra_ctx);
        assert!(result.is_ok());

        let ret = get_pods(&infra_ctx, Kind::Scw, &environment, selector);
        assert!(ret.is_ok());
        assert!(!ret.unwrap().items.is_empty());

        let result = environment.pause_environment(&ea, &infra_ctx_for_delete);
        assert!(result.is_ok());

        // Check that we have actually 0 pods running for this app
        let ret = get_pods(&infra_ctx, Kind::Scw, &environment, selector);
        assert!(ret.is_ok());
        assert!(ret.unwrap().items.is_empty());

        // Check we can resume the env
        let ctx_resume = context.clone_not_same_execution_id();
        let infra_ctx_resume =
            scw_infra_config(&target_cluster_scw_test, &ctx_resume, logger.clone(), metrics_registry.clone());
        let result = environment.deploy_environment(&ea, &infra_ctx_resume);
        assert!(result.is_ok());

        let ret = get_pods(&infra_ctx, Kind::Scw, &environment, selector);
        assert!(ret.is_ok());
        assert!(!ret.unwrap().items.is_empty());

        // Cleanup
        let result = environment.delete_environment(&ea, &infra_ctx_for_delete);
        assert!(result.is_ok());
        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn scaleway_kapsule_redeploy_same_app() {
    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID"),
        );
        let region = ScwZone::from_str(
            secrets
                .SCALEWAY_TEST_CLUSTER_REGION
                .as_ref()
                .expect("SCALEWAY_TEST_CLUSTER_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown SCW region");
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry.clone());
        let context_bis = context.clone_not_same_execution_id();
        let infra_ctx_bis =
            scw_infra_config(&target_cluster_scw_test, &context_bis, logger.clone(), metrics_registry.clone());
        let context_for_deletion = context.clone_not_same_execution_id();
        let infra_ctx_for_deletion = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_deletion,
            logger.clone(),
            metrics_registry.clone(),
        );

        let mut environment = helpers::environment::working_minimal_environment(&context);

        let storage_size: u32 = 10;
        environment.applications = environment
            .applications
            .into_iter()
            .map(|mut app| {
                let id = Uuid::new_v4();
                app.storage = vec![Storage {
                    id: to_short_id(&id),
                    long_id: id,
                    name: "photos".to_string(),
                    storage_class: "scw-sbv-ssd-0".to_string(),
                    size_in_gib: storage_size,
                    mount_point: "/mnt/photos".to_string(),
                    snapshot_retention_in_days: 0,
                }];
                app
            })
            .collect::<Vec<qovery_engine::io_models::application::Application>>();

        let environment_redeploy = environment.clone();
        let environment_check1 = environment.clone();
        let environment_check2 = environment.clone();
        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;

        let env_action = environment.clone();
        let env_action_redeploy = environment_redeploy.clone();
        let env_action_delete = environment_delete.clone();

        let result = environment.deploy_environment(&env_action, &infra_ctx);
        assert!(result.is_ok());

        match get_pvc(&infra_ctx, Kind::Scw, &environment) {
            Ok(pvc) => assert_eq!(
                pvc.items.expect("No items in pvc")[0].spec.resources.requests.storage,
                format!("{storage_size}Gi")
            ),
            Err(_) => panic!(),
        };

        let (_, number) = is_pod_restarted_env(
            &infra_ctx,
            Kind::Scw,
            &environment_check1,
            &environment_check1.applications[0].long_id,
        );

        let result = environment_redeploy.deploy_environment(&env_action_redeploy, &infra_ctx_bis);
        assert!(result.is_ok());

        let (_, number2) = is_pod_restarted_env(
            &infra_ctx,
            Kind::Scw,
            &environment_check2,
            &environment_check2.applications[0].long_id,
        );

        // nothing changed in the app, so, it shouldn't be restarted
        assert!(number.eq(&number2));

        let result = environment_delete.delete_environment(&env_action_delete, &infra_ctx_for_deletion);
        assert!(result.is_ok());

        if let Err(e) = clean_environments(&context, vec![environment], region) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn scaleway_kapsule_deploy_a_not_working_environment_and_then_working_environment() {
    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name,);
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID"),
        );
        let region = ScwZone::from_str(
            secrets
                .SCALEWAY_TEST_CLUSTER_REGION
                .as_ref()
                .expect("SCALEWAY_TEST_CLUSTER_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown SCW region");
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_not_working = context.clone_not_same_execution_id();
        let infra_ctx_for_not_working = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_not_working,
            logger.clone(),
            metrics_registry.clone(),
        );
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
        );

        // env part generation
        let environment = helpers::environment::working_minimal_environment(&context);
        let mut environment_for_not_working = environment.clone();
        // this environment is broken by container exit
        environment_for_not_working.applications = environment_for_not_working
            .applications
            .into_iter()
            .map(|mut app| {
                app.git_url = "https://github.com/Qovery/engine-testing.git".to_string();
                app.branch = "1app_fail_deploy".to_string();
                app.commit_id = "5b89305b9ae8a62a1f16c5c773cddf1d12f70db1".to_string();
                app.environment_vars_with_infos = BTreeMap::default();
                app
            })
            .collect::<Vec<qovery_engine::io_models::application::Application>>();

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // environment actions
        let env_action = environment.clone();
        let env_action_not_working = environment_for_not_working.clone();
        let env_action_delete = environment_for_delete.clone();

        let result =
            environment_for_not_working.deploy_environment(&env_action_not_working, &infra_ctx_for_not_working);
        assert!(result.is_err());

        let result = environment.deploy_environment(&env_action, &infra_ctx);
        assert!(result.is_ok());

        let result = environment_for_delete.delete_environment(&env_action_delete, &infra_ctx_for_delete);
        assert!(result.is_ok());

        if let Err(e) = clean_environments(&context, vec![environment], region) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
#[ignore] // TODO(benjaminch): Make it work (it doesn't work on AWS neither)
fn scaleway_kapsule_deploy_ok_fail_fail_ok_environment() {
    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        // working env

        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID"),
        );
        let region = ScwZone::from_str(
            secrets
                .SCALEWAY_TEST_CLUSTER_REGION
                .as_ref()
                .expect("SCALEWAY_TEST_CLUSTER_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown SCW region");
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry.clone());
        let environment = helpers::environment::working_minimal_environment(&context);

        // not working 1
        let context_for_not_working_1 = context.clone_not_same_execution_id();
        let infra_ctx_for_not_working_1 = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_not_working_1,
            logger.clone(),
            metrics_registry.clone(),
        );
        let mut not_working_env_1 = environment.clone();
        not_working_env_1.applications = not_working_env_1
            .applications
            .into_iter()
            .map(|mut app| {
                app.git_url = "https://gitlab.com/maathor/my-exit-container".to_string();
                app.branch = "master".to_string();
                app.commit_id = "55bc95a23fbf91a7699c28c5f61722d4f48201c9".to_string();
                app.environment_vars_with_infos = BTreeMap::default();
                app
            })
            .collect::<Vec<qovery_engine::io_models::application::Application>>();

        // not working 2
        let context_for_not_working_2 = context.clone_not_same_execution_id();
        let infra_ctx_for_not_working_2 = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_not_working_2,
            logger.clone(),
            metrics_registry.clone(),
        );
        let not_working_env_2 = not_working_env_1.clone();

        // work for delete
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
        );
        let mut delete_env = environment.clone();
        delete_env.action = Action::Delete;

        let env_action = environment.clone();
        let env_action_not_working_1 = not_working_env_1.clone();
        let env_action_not_working_2 = not_working_env_2.clone();
        let env_action_delete = delete_env.clone();

        // OK
        let result = environment.deploy_environment(&env_action, &infra_ctx);
        assert!(result.is_ok());

        // FAIL and rollback
        let result = not_working_env_1.deploy_environment(&env_action_not_working_1, &infra_ctx_for_not_working_1);
        assert!(result.is_err());

        // FAIL and Rollback again
        let result = not_working_env_2.deploy_environment(&env_action_not_working_2, &infra_ctx_for_not_working_2);
        assert!(result.is_err());

        // Should be working
        let result = environment.deploy_environment(&env_action, &infra_ctx);
        assert!(result.is_ok());

        let result = delete_env.delete_environment(&env_action_delete, &infra_ctx_for_delete);
        assert!(result.is_ok());

        if let Err(e) = clean_environments(&context, vec![environment], region) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn scaleway_kapsule_deploy_a_non_working_environment_with_no_failover() {
    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name,);
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID"),
        );
        let region = ScwZone::from_str(
            secrets
                .SCALEWAY_TEST_CLUSTER_REGION
                .as_ref()
                .expect("SCALEWAY_TEST_CLUSTER_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown SCW region");
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry.clone());
        let environment = helpers::environment::non_working_environment(&context);

        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
        );
        let mut delete_env = environment.clone();
        delete_env.action = Action::Delete;

        let env_action = environment.clone();
        let env_action_delete = delete_env.clone();

        let result = environment.deploy_environment(&env_action, &infra_ctx);
        assert!(result.is_err());

        let result = delete_env.delete_environment(&env_action_delete, &infra_ctx_for_delete);
        assert!(result.is_ok());

        if let Err(e) = clean_environments(&context, vec![environment], region) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[ignore] // TODO: fix main ingress to let it handle sticky session
#[named]
#[test]
fn scaleway_kapsule_deploy_a_working_environment_with_sticky_session() {
    use qovery_engine::environment::models::router::RouterAdvancedSettings;

    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID"),
        );
        let region = ScwZone::from_str(
            secrets
                .SCALEWAY_TEST_CLUSTER_REGION
                .as_ref()
                .expect("SCALEWAY_TEST_CLUSTER_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown SCW region");
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
        );
        let environment = helpers::environment::environment_only_http_server_router_with_sticky_session(
            &context,
            secrets
                .DEFAULT_TEST_DOMAIN
                .as_ref()
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let env_action = environment.clone();
        let env_action_for_delete = environment_for_delete.clone();

        let result = environment.deploy_environment(&env_action, &infra_ctx);
        assert!(result.is_ok());

        // checking cookie is properly set on the app
        let kubeconfig = infra_ctx.kubernetes().kubeconfig_local_file_path();
        let router = environment
            .routers
            .first()
            .unwrap()
            .to_router_domain(
                infra_ctx.context(),
                RouterAdvancedSettings::new(None, None, None),
                infra_ctx.cloud_provider(),
                vec![],
                vec![],
            )
            .unwrap();
        let environment_domain = environment
            .to_environment_domain(
                infra_ctx.context(),
                infra_ctx.cloud_provider(),
                infra_ctx.container_registry(),
                infra_ctx.kubernetes(),
            )
            .unwrap();

        // let some time for ingress to get its IP or hostname
        // Sticky session is checked on ingress IP or hostname so we are not subjects to long DNS propagation making test less flacky.
        let ingress = retry::retry(Fibonacci::from_millis(15000).take(8), || {
            match qovery_engine::cmd::kubectl::kubectl_exec_get_external_ingress(
                &kubeconfig,
                environment_domain.namespace(),
                router.kube_name(),
                infra_ctx.cloud_provider().credentials_environment_variables(),
            ) {
                Ok(res) => match res {
                    Some(res) => retry::OperationResult::Ok(res),
                    None => retry::OperationResult::Retry("ingress not found"),
                },
                Err(_) => retry::OperationResult::Retry("cannot get ingress"),
            }
        })
        .expect("cannot get ingress");
        let ingress_host = ingress
            .ip
            .as_ref()
            .unwrap_or_else(|| ingress.hostname.as_ref().expect("ingress has no IP nor hostname"));

        for router in environment.routers.iter() {
            for route in router.routes.iter() {
                assert!(session_is_sticky(
                    Url::parse(format!("http://{}{}", ingress_host, route.path).as_str()).expect("cannot parse URL"),
                    router.default_domain.clone(),
                    85400,
                ));
            }
        }

        let result = environment_for_delete.delete_environment(&env_action_for_delete, &infra_ctx_for_delete);
        assert!(result.is_ok());

        if let Err(e) = clean_environments(&context, vec![environment], region) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn scaleway_kapsule_deploy_a_working_environment_with_ip_whitelist_allowing_all() {
    use qovery_engine::environment::models::router::RouterAdvancedSettings;

    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID"),
        );
        let region = ScwZone::from_str(
            secrets
                .SCALEWAY_TEST_CLUSTER_REGION
                .as_ref()
                .expect("SCALEWAY_TEST_CLUSTER_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown SCW region");
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
        );
        let whitelist_all_environment =
            helpers::environment::environment_only_http_server_router_with_ip_whitelist_source_range(
                &context,
                secrets
                    .DEFAULT_TEST_DOMAIN
                    .as_ref()
                    .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                    .as_str(),
                Some("0.0.0.0/0".to_string()), // <- Allow all IPs
            );

        let mut whitelist_all_environment_for_delete = whitelist_all_environment.clone();
        whitelist_all_environment_for_delete.action = Action::Delete;

        let env_action = whitelist_all_environment.clone();
        let env_action_for_delete = whitelist_all_environment_for_delete.clone();

        let result = whitelist_all_environment.deploy_environment(&env_action, &infra_ctx);
        assert!(result.is_ok());

        let kubeconfig = infra_ctx.kubernetes().kubeconfig_local_file_path();
        let router = whitelist_all_environment
            .routers
            .first()
            .unwrap()
            .to_router_domain(
                infra_ctx.context(),
                RouterAdvancedSettings::new(None, None, None),
                infra_ctx.cloud_provider(),
                vec![],
                vec![],
            )
            .unwrap();
        let environment_domain = whitelist_all_environment
            .to_environment_domain(
                infra_ctx.context(),
                infra_ctx.cloud_provider(),
                infra_ctx.container_registry(),
                infra_ctx.kubernetes(),
            )
            .unwrap();

        // let some time for ingress to get its IP or hostname
        let ingress = retry::retry(Fibonacci::from_millis(15000).take(8), || {
            match qovery_engine::cmd::kubectl::kubectl_exec_get_external_ingress(
                &kubeconfig,
                environment_domain.namespace(),
                router.kube_name(),
                infra_ctx.cloud_provider().credentials_environment_variables(),
            ) {
                Ok(res) => match res {
                    Some(res) => retry::OperationResult::Ok(res),
                    None => retry::OperationResult::Retry("ingress not found"),
                },
                Err(_) => retry::OperationResult::Retry("cannot get ingress"),
            }
        })
        .expect("cannot get ingress");
        let ingress_host = ingress
            .ip
            .as_ref()
            .unwrap_or_else(|| ingress.hostname.as_ref().expect("ingress has no IP nor hostname"));

        let http_client = reqwest::blocking::Client::builder()
            .danger_accept_invalid_certs(true) // this test ignores certificate validity (not its purpose)
            .build()
            .expect("Cannot build reqwest client");

        for router in whitelist_all_environment.routers.iter() {
            for route in router.routes.iter() {
                let http_request_result = http_client
                    .get(
                        Url::parse(format!("http://{}{}", ingress_host, route.path).as_str())
                            .expect("cannot parse URL")
                            .to_string(),
                    )
                    .header("Host", router.default_domain.as_str())
                    .send()
                    .expect("Cannot get HTTP response");

                assert_eq!(StatusCode::OK, http_request_result.status());
            }
        }

        let result =
            whitelist_all_environment_for_delete.delete_environment(&env_action_for_delete, &infra_ctx_for_delete);
        assert!(result.is_ok());

        if let Err(e) = clean_environments(&context, vec![whitelist_all_environment], region) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn scaleway_kapsule_deploy_a_working_environment_with_ip_whitelist_deny_all() {
    use qovery_engine::environment::models::router::RouterAdvancedSettings;

    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID"),
        );
        let region = ScwZone::from_str(
            secrets
                .SCALEWAY_TEST_CLUSTER_REGION
                .as_ref()
                .expect("SCALEWAY_TEST_CLUSTER_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown SCW region");
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
        );
        let whitelist_all_environment =
            helpers::environment::environment_only_http_server_router_with_ip_whitelist_source_range(
                &context,
                secrets
                    .DEFAULT_TEST_DOMAIN
                    .as_ref()
                    .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                    .as_str(),
                Some("0.0.0.0/32".to_string()), // <- deny all IPs
            );

        let mut deny_all_environment_for_delete = whitelist_all_environment.clone();
        deny_all_environment_for_delete.action = Action::Delete;

        let env_action = whitelist_all_environment.clone();
        let env_action_for_delete = deny_all_environment_for_delete.clone();

        let result = whitelist_all_environment.deploy_environment(&env_action, &infra_ctx);
        assert!(result.is_ok());

        // checking cookie is properly set on the app
        let kubeconfig = infra_ctx.kubernetes().kubeconfig_local_file_path();
        let router = whitelist_all_environment
            .routers
            .first()
            .unwrap()
            .to_router_domain(
                infra_ctx.context(),
                RouterAdvancedSettings::new(None, None, None),
                infra_ctx.cloud_provider(),
                vec![],
                vec![],
            )
            .unwrap();
        let environment_domain = whitelist_all_environment
            .to_environment_domain(
                infra_ctx.context(),
                infra_ctx.cloud_provider(),
                infra_ctx.container_registry(),
                infra_ctx.kubernetes(),
            )
            .unwrap();

        // let some time for ingress to get its IP or hostname
        let ingress = retry::retry(Fibonacci::from_millis(15000).take(8), || {
            match qovery_engine::cmd::kubectl::kubectl_exec_get_external_ingress(
                &kubeconfig,
                environment_domain.namespace(),
                router.kube_name(),
                infra_ctx.cloud_provider().credentials_environment_variables(),
            ) {
                Ok(res) => match res {
                    Some(res) => retry::OperationResult::Ok(res),
                    None => retry::OperationResult::Retry("ingress not found"),
                },
                Err(_) => retry::OperationResult::Retry("cannot get ingress"),
            }
        })
        .expect("cannot get ingress");
        let ingress_host = ingress
            .ip
            .as_ref()
            .unwrap_or_else(|| ingress.hostname.as_ref().expect("ingress has no IP nor hostname"));

        let http_client = reqwest::blocking::Client::builder()
            .danger_accept_invalid_certs(true) // this test ignores certificate validity (not its purpose)
            .build()
            .expect("Cannot build reqwest client");

        for router in whitelist_all_environment.routers.iter() {
            for route in router.routes.iter() {
                let http_request_result = http_client
                    .get(
                        Url::parse(format!("http://{}{}", ingress_host, route.path).as_str())
                            .expect("cannot parse URL")
                            .to_string(),
                    )
                    .header("Host", router.default_domain.as_str())
                    .send()
                    .expect("Cannot get HTTP response");

                assert_eq!(StatusCode::FORBIDDEN, http_request_result.status());
            }
        }

        let result = deny_all_environment_for_delete.delete_environment(&env_action_for_delete, &infra_ctx_for_delete);
        assert!(result.is_ok());

        if let Err(e) = clean_environments(&context, vec![whitelist_all_environment], region) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_container_with_no_router_on_scw() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
        );

        let mut environment = helpers::environment::working_minimal_environment(&context);

        environment.routers = vec![];
        environment.applications = vec![];
        let service_id = Uuid::new_v4();
        environment.containers = vec![Container {
            long_id: Uuid::new_v4(),
            name: "👾👾👾 my little container 澳大利亚和智利提及年度采购计划 👾👾👾".to_string(),
            kube_name: "my-little-container".to_string(),
            action: Action::Create,
            registry: Registry::PublicEcr {
                long_id: Uuid::new_v4(),
                url: Url::parse("https://public.ecr.aws").unwrap(),
            },
            image: "r3m4q3r9/pub-mirror-debian".to_string(),
            tag: "11.6-ci".to_string(),
            command_args: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                r#"
                apt-get update;
                apt-get install -y socat procps iproute2;
                echo listening on port $PORT;
                env
                socat TCP6-LISTEN:8080,bind=[::],reuseaddr,fork STDOUT
                "#
                .to_string(),
            ],
            entrypoint: None,
            cpu_request_in_milli: 250,
            cpu_limit_in_milli: 250,
            ram_request_in_mib: 250,
            ram_limit_in_mib: 250,
            min_instances: 1,
            max_instances: 1,
            public_domain: format!("{}.{}", service_id, infra_ctx.dns_provider().domain()),
            ports: vec![
                Port {
                    long_id: Uuid::new_v4(),
                    port: 8080,
                    name: "http".to_string(),
                    is_default: true,
                    publicly_accessible: true,
                    protocol: Protocol::HTTP,
                    service_name: None,
                    namespace: None,
                    additional_service: None,
                },
                Port {
                    long_id: Uuid::new_v4(),
                    port: 8081,
                    name: "grpc".to_string(),
                    is_default: false,
                    publicly_accessible: false,
                    protocol: Protocol::HTTP,
                    service_name: None,
                    namespace: None,
                    additional_service: None,
                },
            ],
            readiness_probe: Some(Probe {
                r#type: ProbeType::Tcp { host: None },
                port: 8080,
                initial_delay_seconds: 1,
                timeout_seconds: 2,
                period_seconds: 3,
                success_threshold: 1,
                failure_threshold: 5,
            }),
            liveness_probe: Some(Probe {
                r#type: ProbeType::Tcp { host: None },
                port: 8080,
                initial_delay_seconds: 1,
                timeout_seconds: 2,
                period_seconds: 3,
                success_threshold: 1,
                failure_threshold: 5,
            }),
            storages: vec![],
            environment_vars_with_infos: btreemap! { "MY_VAR".to_string() => VariableInfo{ value: general_purpose::STANDARD.encode("my_value"), is_secret: false} },
            mounted_files: vec![],
            advanced_settings: Default::default(),
            annotations_group_ids: BTreeSet::new(),
            labels_group_ids: btreeset! {},
        }];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok());

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_container_on_scw_with_mounted_files_as_volume() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
        );

        let mut environment = helpers::environment::working_minimal_environment(&context);

        let mounted_file_identifier = QoveryIdentifier::new_random();
        let mounted_file = MountedFile {
            id: mounted_file_identifier.short().to_string(),
            long_id: mounted_file_identifier.to_uuid(),
            mount_path: "/this-file-should-exist".to_string(),
            file_content_b64: general_purpose::STANDARD.encode("I exist !"),
        };

        environment.routers = vec![];
        environment.applications = vec![];
        let service_id = Uuid::new_v4();
        environment.containers = vec![Container {
            long_id: Uuid::new_v4(),
            name: "👾👾👾 my little container 澳大利亚和智利提及年度采购计划 👾👾👾".to_string(),
            kube_name: "my-little-container".to_string(),
            action: Action::Create,
            registry: Registry::PublicEcr {
                long_id: Uuid::new_v4(),
                url: Url::parse("https://public.ecr.aws").unwrap(),
            },
            image: "r3m4q3r9/pub-mirror-debian".to_string(),
            tag: "11.6-ci".to_string(),
            command_args: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                format!(
                    r#"
                apt-get update;
                apt-get install -y socat procps iproute2;
                echo listening on port $PORT;
                env
                cat {}
                socat TCP6-LISTEN:8080,bind=[::],reuseaddr,fork STDOUT
                "#,
                    &mounted_file.mount_path
                ),
            ],
            entrypoint: None,
            cpu_request_in_milli: 250,
            cpu_limit_in_milli: 250,
            ram_request_in_mib: 250,
            ram_limit_in_mib: 250,
            min_instances: 1,
            max_instances: 1,
            public_domain: format!("{}.{}", service_id, infra_ctx.dns_provider().domain()),
            ports: vec![
                Port {
                    long_id: Uuid::new_v4(),
                    port: 8080,
                    name: "http".to_string(),
                    is_default: true,
                    publicly_accessible: true,
                    protocol: Protocol::HTTP,
                    service_name: None,
                    namespace: None,
                    additional_service: None,
                },
                Port {
                    long_id: Uuid::new_v4(),
                    port: 8081,
                    name: "grpc".to_string(),
                    is_default: false,
                    publicly_accessible: false,
                    protocol: Protocol::HTTP,
                    service_name: None,
                    namespace: None,
                    additional_service: None,
                },
            ],
            storages: vec![],
            environment_vars_with_infos: btreemap! { "MY_VAR".to_string() =>  VariableInfo{ value: general_purpose::STANDARD.encode("my_value"), is_secret: false} },
            mounted_files: vec![mounted_file.clone()],
            advanced_settings: Default::default(),
            readiness_probe: Some(Probe {
                r#type: ProbeType::Tcp { host: None },
                port: 8080,
                initial_delay_seconds: 1,
                timeout_seconds: 2,
                period_seconds: 3,
                success_threshold: 1,
                failure_threshold: 5,
            }),
            liveness_probe: Some(Probe {
                r#type: ProbeType::Tcp { host: None },
                port: 8080,
                initial_delay_seconds: 1,
                timeout_seconds: 2,
                period_seconds: 3,
                success_threshold: 1,
                failure_threshold: 5,
            }),
            annotations_group_ids: BTreeSet::new(),
            labels_group_ids: btreeset! {},
        }];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok());

        // check if secret exists
        let service_id = QoveryIdentifier::new(
            environment
                .containers
                .first()
                .expect("there must be at least one container in environment")
                .long_id,
        )
        .short()
        .to_string();
        let config_maps = kubectl_get_secret(
            infra_ctx.mk_kube_client().expect("kube client is not set").client(),
            format!("metadata.name={}-{}", &mounted_file.id, service_id).as_str(),
        )
        .expect("unable to find secret for selector");
        assert!(!config_maps.is_empty());
        for cm in config_maps {
            assert_eq!(
                general_purpose::STANDARD
                    .decode(&mounted_file.file_content_b64)
                    .expect("mounted file content cannot be b64 decoded")
                    .to_str(),
                cm.data
                    .expect("data should be set")
                    .get("content")
                    .expect("content should exist")
                    .0
                    .to_str()
            );
        }

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn deploy_container_with_router_on_scw() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
        );

        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();
        let test_domain = secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str();

        environment.applications = vec![];
        let service_id = Uuid::new_v4();
        let annotations_group_id = Uuid::new_v4();
        let labels_group_id = Uuid::new_v4();
        environment.containers = vec![Container {
            long_id: service_id,
            name: "👾👾👾 my little container 澳大利亚和智利提及年度采购计划 👾👾👾".to_string(),
            kube_name: format!("my-little-container-{suffix}"),
            action: Action::Create,
            registry: Registry::DockerHub {
                url: Url::parse("https://public.ecr.aws").unwrap(),
                long_id: Uuid::new_v4(),
                credentials: None,
            },
            image: "r3m4q3r9/pub-mirror-httpd".to_string(),
            tag: "2.4.56-alpine3.17".to_string(),
            command_args: vec![],
            entrypoint: None,
            cpu_request_in_milli: 250,
            cpu_limit_in_milli: 250,
            ram_request_in_mib: 250,
            ram_limit_in_mib: 250,
            min_instances: 1,
            max_instances: 1,
            public_domain: format!("{}.{}", service_id, infra_ctx.dns_provider().domain()),
            ports: vec![
                Port {
                    long_id: Uuid::new_v4(),
                    port: 80,
                    name: "http".to_string(),
                    is_default: true,
                    publicly_accessible: true,
                    protocol: Protocol::HTTP,
                    service_name: None,
                    namespace: None,
                    additional_service: None,
                },
                Port {
                    long_id: Uuid::new_v4(),
                    port: 8081,
                    name: "grpc".to_string(),
                    is_default: false,
                    publicly_accessible: false,
                    protocol: Protocol::HTTP,
                    service_name: None,
                    namespace: None,
                    additional_service: None,
                },
            ],
            storages: vec![],
            environment_vars_with_infos: btreemap! { "MY_VAR".to_string() => VariableInfo{ value:general_purpose::STANDARD.encode("my_value"), is_secret: false} },
            mounted_files: vec![],
            advanced_settings: Default::default(),
            readiness_probe: Some(Probe {
                r#type: ProbeType::Tcp { host: None },
                port: 80,
                initial_delay_seconds: 1,
                timeout_seconds: 2,
                period_seconds: 3,
                success_threshold: 1,
                failure_threshold: 5,
            }),
            liveness_probe: Some(Probe {
                r#type: ProbeType::Tcp { host: None },
                port: 80,
                initial_delay_seconds: 1,
                timeout_seconds: 2,
                period_seconds: 3,
                success_threshold: 1,
                failure_threshold: 5,
            }),
            annotations_group_ids: btreeset! { annotations_group_id },
            labels_group_ids: btreeset! { labels_group_id },
        }];
        environment.annotations_groups = btreemap! { annotations_group_id => AnnotationsGroup {
            annotations: vec![Annotation {
                key: "annot_key".to_string(),
                value: "annot_value".to_string(),
            }],
            scopes: vec![
                AnnotationsGroupScope::Deployments,
                AnnotationsGroupScope::Services,
                AnnotationsGroupScope::Ingress,
                AnnotationsGroupScope::Hpa,
                AnnotationsGroupScope::Pods,
                AnnotationsGroupScope::Secrets,
            ],
        }};
        environment.labels_groups = btreemap! { labels_group_id => LabelsGroup {
            labels: vec![Label {
                key: "label_key".to_string(),
                value: "label_value".to_string(),
                propagate_to_cloud_provider: false,
            }]
        }};

        environment.routers = vec![Router {
            long_id: Uuid::new_v4(),
            name: "default-router".to_string(),
            kube_name: format!("router-{suffix}"),
            action: Action::Create,
            default_domain: format!("main.{}.{}", context.cluster_short_id(), test_domain),
            public_port: 443,
            custom_domains: vec![],
            routes: vec![Route {
                path: "/".to_string(),
                service_long_id: environment.containers[0].long_id,
            }],
        }];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok());

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[test]
fn deploy_job_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = "deploy_job_on_scw_kapsule");
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
        );

        let mut environment = helpers::environment::working_minimal_environment(&context);

        let json_output = "{\"foo\": {\"value\": \"bar\", \"sensitive\": true}, \"foo_2\": {\"value\": \"bar_2\"}}";
        //environment.long_id = Uuid::default();
        //environment.project_long_id = Uuid::default();
        let annotations_group_id = Uuid::new_v4();
        let labels_group_id = Uuid::new_v4();
        environment.applications = vec![];
        environment.jobs = vec![Job {
            long_id: Uuid::new_v4(), //Uuid::default(),
            name: "job test #####".to_string(),
            kube_name: "job-test".to_string(),
            action: Action::Create,
            schedule: JobSchedule::OnStart {
                lifecycle_type: LifecycleType::GENERIC,
            }, //JobSchedule::Cron("* * * * *".to_string()),
            source: JobSource::Image {
                registry: Registry::PublicEcr {
                    long_id: Uuid::new_v4(),
                    url: Url::parse("https://public.ecr.aws").unwrap(),
                },
                image: "r3m4q3r9/pub-mirror-debian".to_string(),
                tag: "11.6-ci".to_string(),
            },
            max_nb_restart: 2,
            max_duration_in_sec: 300,
            default_port: Some(8080),
            //command_args: vec![],
            command_args: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                format!("echo starting; sleep 10; echo '{json_output}' > /qovery-output/qovery-output.json"),
            ],
            entrypoint: None,
            force_trigger: false,
            cpu_request_in_milli: 100,
            cpu_limit_in_milli: 100,
            ram_request_in_mib: 100,
            ram_limit_in_mib: 100,
            environment_vars_with_infos: Default::default(),
            mounted_files: vec![],
            advanced_settings: Default::default(),
            readiness_probe: Some(Probe {
                r#type: ProbeType::Tcp { host: None },
                port: 8080,
                initial_delay_seconds: 1,
                timeout_seconds: 2,
                period_seconds: 3,
                success_threshold: 1,
                failure_threshold: 5,
            }),
            liveness_probe: Some(Probe {
                r#type: ProbeType::Tcp { host: None },
                port: 8080,
                initial_delay_seconds: 1,
                timeout_seconds: 2,
                period_seconds: 3,
                success_threshold: 1,
                failure_threshold: 5,
            }),
            container_registries: ContainerRegistries { registries: vec![] },
            annotations_group_ids: btreeset! { annotations_group_id },
            labels_group_ids: btreeset! {labels_group_id},
            should_delete_shared_registry: false,
            shared_image_feature_enabled: false,
            output_variable_validation_pattern: "^[a-zA-Z_][a-zA-Z0-9_]*$".to_string(),
        }];
        environment.annotations_groups = btreemap! { annotations_group_id => AnnotationsGroup {
            annotations: vec![Annotation {
                key: "annot_key".to_string(),
                value: "annot_value".to_string(),
            }],
            scopes: vec![
                AnnotationsGroupScope::Deployments,
                AnnotationsGroupScope::Services,
                AnnotationsGroupScope::Ingress,
                AnnotationsGroupScope::Hpa,
                AnnotationsGroupScope::Pods,
                AnnotationsGroupScope::Secrets,
            ],
        }};
        environment.labels_groups = btreemap! { labels_group_id => LabelsGroup {
            labels: vec![Label {
                key: "label_key".to_string(),
                value: "label_value".to_string(),
                propagate_to_cloud_provider: false,
            }],
        }};

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok());

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[test]
fn deploy_cronjob_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = "deploy_cronjob_on_scw_kapsule");
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
        );

        let mut environment = helpers::environment::working_minimal_environment(&context);

        environment.applications = vec![];
        environment.jobs = vec![Job {
            long_id: Uuid::new_v4(),
            name: "job test #####||||*_-(".to_string(),
            kube_name: "job-test".to_string(),
            action: Action::Create,
            schedule: JobSchedule::Cron {
                schedule: "* * * * *".to_string(),
                timezone: "Etc/UTC".to_string(),
            },
            source: JobSource::Image {
                registry: Registry::PublicEcr {
                    long_id: Uuid::new_v4(),
                    url: Url::parse("https://public.ecr.aws").unwrap(),
                },
                image: "r3m4q3r9/pub-mirror-debian".to_string(),
                tag: "11.6-ci".to_string(),
            },
            max_nb_restart: 1,
            max_duration_in_sec: 30,
            default_port: Some(8080),
            command_args: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "echo starting; sleep 10; echo started".to_string(),
            ],
            entrypoint: None,
            force_trigger: false,
            cpu_request_in_milli: 100,
            cpu_limit_in_milli: 100,
            ram_request_in_mib: 100,
            ram_limit_in_mib: 100,
            environment_vars_with_infos: Default::default(),
            mounted_files: vec![],
            advanced_settings: Default::default(),
            readiness_probe: Some(Probe {
                r#type: ProbeType::Tcp { host: None },
                port: 8080,
                initial_delay_seconds: 1,
                timeout_seconds: 2,
                period_seconds: 3,
                success_threshold: 1,
                failure_threshold: 5,
            }),
            liveness_probe: Some(Probe {
                r#type: ProbeType::Tcp { host: None },
                port: 8080,
                initial_delay_seconds: 1,
                timeout_seconds: 2,
                period_seconds: 3,
                success_threshold: 1,
                failure_threshold: 5,
            }),
            container_registries: ContainerRegistries { registries: vec![] },
            annotations_group_ids: btreeset! {},
            labels_group_ids: btreeset! {},
            should_delete_shared_registry: false,
            shared_image_feature_enabled: false,
            output_variable_validation_pattern: "^[a-zA-Z_][a-zA-Z0-9_]*$".to_string(),
        }];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok());

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[test]
fn deploy_cronjob_force_trigger_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = "deploy_cronjob_on_scw_kapsule");
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
        );

        let mut environment = helpers::environment::working_minimal_environment(&context);

        environment.applications = vec![];
        environment.jobs = vec![Job {
            long_id: Uuid::new_v4(),
            name: "job test #####||||*_-(".to_string(),
            kube_name: "job-test".to_string(),
            action: Action::Create,
            schedule: JobSchedule::Cron {
                schedule: "*/10 * * * *".to_string(),
                timezone: "Etc/UTC".to_string(),
            },
            source: JobSource::Image {
                registry: Registry::PublicEcr {
                    long_id: Uuid::new_v4(),
                    url: Url::parse("https://public.ecr.aws").unwrap(),
                },
                image: "r3m4q3r9/pub-mirror-debian".to_string(),
                tag: "11.6-ci".to_string(),
            },
            max_nb_restart: 1,
            max_duration_in_sec: 30,
            default_port: Some(8080),
            command_args: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "echo starting; sleep 10; echo started".to_string(),
            ],
            entrypoint: None,
            force_trigger: true,
            cpu_request_in_milli: 100,
            cpu_limit_in_milli: 100,
            ram_request_in_mib: 100,
            ram_limit_in_mib: 100,
            environment_vars_with_infos: Default::default(),
            mounted_files: vec![],
            advanced_settings: Default::default(),
            readiness_probe: Some(Probe {
                r#type: ProbeType::Tcp { host: None },
                port: 8080,
                initial_delay_seconds: 1,
                timeout_seconds: 2,
                period_seconds: 3,
                success_threshold: 1,
                failure_threshold: 5,
            }),
            liveness_probe: Some(Probe {
                r#type: ProbeType::Tcp { host: None },
                port: 8080,
                initial_delay_seconds: 1,
                timeout_seconds: 2,
                period_seconds: 3,
                success_threshold: 1,
                failure_threshold: 5,
            }),
            container_registries: ContainerRegistries { registries: vec![] },
            annotations_group_ids: btreeset! {},
            labels_group_ids: btreeset! {},
            should_delete_shared_registry: false,
            shared_image_feature_enabled: false,
            output_variable_validation_pattern: "^[a-zA-Z_][a-zA-Z0-9_]*$".to_string(),
        }];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok());

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[test]
fn build_and_deploy_job_on_scw_kapsule() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = "build_and_deploy_job_on_scw_kapsule");
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
        );

        let mut environment = helpers::environment::working_minimal_environment(&context);

        let json_output = "{\"foo\": {\"value\": \"bar\", \"sensitive\": true}, \"foo_2\": {\"value\": \"bar_2\"}}";
        environment.applications = vec![];
        environment.jobs = vec![Job {
            long_id: Uuid::new_v4(),
            name: "job test #####".to_string(),
            kube_name: "job-test".to_string(),
            action: Action::Create,
            schedule: JobSchedule::OnStart {
                lifecycle_type: LifecycleType::GENERIC,
            },
            source: JobSource::Docker {
                git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
                commit_id: "fc575a2f3be0b9100492c8a463bf18134a8698a5".to_string(),
                dockerfile_path: Some("Dockerfile".to_string()),
                root_path: String::from("/"),
                git_credentials: None,
                branch: "main".to_string(),
                dockerfile_content: None,
                docker_target_build_stage: None,
            },
            max_nb_restart: 2,
            max_duration_in_sec: 300,
            default_port: Some(8080),
            //command_args: vec![],
            command_args: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                format!("echo starting; sleep 10; echo '{json_output}' > /qovery-output/qovery-output.json"),
            ],
            entrypoint: None,
            force_trigger: false,
            cpu_request_in_milli: 100,
            cpu_limit_in_milli: 100,
            ram_request_in_mib: 100,
            ram_limit_in_mib: 100,
            environment_vars_with_infos: Default::default(),
            mounted_files: vec![],
            advanced_settings: Default::default(),
            readiness_probe: Some(Probe {
                r#type: ProbeType::Tcp { host: None },
                port: 8080,
                initial_delay_seconds: 1,
                timeout_seconds: 2,
                period_seconds: 3,
                success_threshold: 1,
                failure_threshold: 5,
            }),
            liveness_probe: Some(Probe {
                r#type: ProbeType::Tcp { host: None },
                port: 8080,
                initial_delay_seconds: 1,
                timeout_seconds: 2,
                period_seconds: 3,
                success_threshold: 1,
                failure_threshold: 5,
            }),
            container_registries: ContainerRegistries { registries: vec![] },
            annotations_group_ids: btreeset! {},
            labels_group_ids: btreeset! {},
            should_delete_shared_registry: false,
            shared_image_feature_enabled: false,
            output_variable_validation_pattern: "^[a-zA-Z_][a-zA-Z0-9_]*$".to_string(),
        }];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok());

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[test]
fn build_and_deploy_job_on_scw_kapsule_with_mounted_files() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = "build_and_deploy_job_on_scw_kapsule");
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID is not set"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
        );

        let mounted_file_identifier = QoveryIdentifier::new_random();
        let mounted_file = MountedFile {
            id: mounted_file_identifier.short().to_string(),
            long_id: mounted_file_identifier.to_uuid(),
            mount_path: "/this-file-should-exist.json".to_string(),
            file_content_b64: general_purpose::STANDARD
                .encode("{\"foo\": {\"value\": \"bar\", \"sensitive\": true}, \"foo_2\": {\"value\": \"bar_2\"}}"),
        };

        let mut environment = helpers::environment::working_minimal_environment(&context);

        environment.applications = vec![];
        environment.jobs = vec![Job {
            long_id: Uuid::new_v4(),
            name: "job test #####".to_string(),
            kube_name: "job-test".to_string(),
            action: Action::Create,
            schedule: JobSchedule::OnStart {
                lifecycle_type: LifecycleType::GENERIC,
            },
            source: JobSource::Docker {
                git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
                commit_id: "fc575a2f3be0b9100492c8a463bf18134a8698a5".to_string(),
                dockerfile_path: Some("Dockerfile".to_string()),
                root_path: String::from("/"),
                git_credentials: None,
                branch: "main".to_string(),
                dockerfile_content: None,
                docker_target_build_stage: None,
            },
            max_nb_restart: 2,
            max_duration_in_sec: 300,
            default_port: Some(8080),
            //command_args: vec![],
            command_args: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                format!(
                    "echo starting; sleep 10; cat {} > /qovery-output/qovery-output.json",
                    &mounted_file.mount_path,
                ),
            ],
            entrypoint: None,
            force_trigger: false,
            cpu_request_in_milli: 100,
            cpu_limit_in_milli: 100,
            ram_request_in_mib: 100,
            ram_limit_in_mib: 100,
            environment_vars_with_infos: Default::default(),
            mounted_files: vec![mounted_file.clone()],
            advanced_settings: Default::default(),
            readiness_probe: Some(Probe {
                r#type: ProbeType::Tcp { host: None },
                port: 8080,
                initial_delay_seconds: 1,
                timeout_seconds: 2,
                period_seconds: 3,
                success_threshold: 1,
                failure_threshold: 5,
            }),
            liveness_probe: Some(Probe {
                r#type: ProbeType::Tcp { host: None },
                port: 8080,
                initial_delay_seconds: 1,
                timeout_seconds: 2,
                period_seconds: 3,
                success_threshold: 1,
                failure_threshold: 5,
            }),
            container_registries: ContainerRegistries { registries: vec![] },
            annotations_group_ids: btreeset! {},
            labels_group_ids: btreeset! {},
            should_delete_shared_registry: false,
            shared_image_feature_enabled: false,
            output_variable_validation_pattern: "^[a-zA-Z_][a-zA-Z0-9_]*$".to_string(),
        }];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok());

        // check if secret exists
        let service_id = QoveryIdentifier::new(
            environment
                .jobs
                .first()
                .expect("there must be at least one job in environment")
                .long_id,
        )
        .short()
        .to_string();
        let config_maps = kubectl_get_secret(
            infra_ctx.mk_kube_client().expect("kube client is not set").client(),
            format!("metadata.name={}-{}", &mounted_file.id, service_id).as_str(),
        )
        .expect("unable to find secret for selector");
        assert!(!config_maps.is_empty());
        for cm in config_maps {
            assert_eq!(
                general_purpose::STANDARD
                    .decode(&mounted_file.file_content_b64)
                    .expect("mounted file content cannot be b64 decoded")
                    .to_str(),
                cm.data
                    .expect("data should be set")
                    .get("content")
                    .expect("content should exist")
                    .0
                    .to_str()
            );
        }

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn deploy_container_with_tcp_public_port() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_LONG_ID
                .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID"),
            secrets
                .SCALEWAY_TEST_CLUSTER_LONG_ID
                .expect("SCALEWAY_TEST_CLUSTER_LONG_ID"),
        );
        let target_cluster_scw_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .SCALEWAY_TEST_KUBECONFIG_b64
                .expect("SCALEWAY_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = scw_infra_config(&target_cluster_scw_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = scw_infra_config(
            &target_cluster_scw_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
        );

        let mut environment = helpers::environment::working_minimal_environment(&context);

        environment.routers = vec![];
        environment.applications = vec![];
        let service_id = Uuid::new_v4();
        environment.containers = vec![Container {
            long_id: service_id,
            name: "👾👾👾 my little container 澳大利亚和智利提及年度采购计划 👾👾👾".to_string(),
            kube_name: "my-little-container".to_string(),
            action: Action::Create,
            registry: Registry::PublicEcr {
                long_id: Uuid::new_v4(),
                url: Url::parse("https://public.ecr.aws").unwrap(),
            },
            image: "r3m4q3r9/pub-mirror-debian".to_string(),
            tag: "11.6-ci".to_string(),
            command_args: vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                r#"
                apt-get update;
                apt-get install -y socat procps iproute2;
                echo listening on port $PORT;
                env
                socat TCP6-LISTEN:5432,bind=[::],reuseaddr,fork STDOUT &
                socat TCP6-LISTEN:443,bind=[::],reuseaddr,fork STDOUT
                "#
                .to_string(),
            ],
            entrypoint: None,
            cpu_request_in_milli: 250,
            cpu_limit_in_milli: 250,
            ram_request_in_mib: 250,
            ram_limit_in_mib: 250,
            min_instances: 1,
            max_instances: 1,
            public_domain: format!("{}.{}", service_id, infra_ctx.dns_provider().domain()),
            ports: vec![
                Port {
                    long_id: Uuid::new_v4(),
                    port: 443,
                    is_default: true,
                    name: "p443".to_string(),
                    publicly_accessible: true,
                    protocol: Protocol::TCP,
                    service_name: None,
                    namespace: None,
                    additional_service: None,
                },
                Port {
                    long_id: Uuid::new_v4(),
                    port: 5432,
                    is_default: false,
                    name: "p5432".to_string(),
                    publicly_accessible: true,
                    protocol: Protocol::TCP,
                    service_name: None,
                    namespace: None,
                    additional_service: None,
                },
                Port {
                    long_id: Uuid::new_v4(),
                    port: 80,
                    is_default: false,
                    name: "p80".to_string(),
                    publicly_accessible: false, // scaleway don't support udp loabalancer
                    protocol: Protocol::UDP,
                    service_name: None,
                    namespace: None,
                    additional_service: None,
                },
            ],
            readiness_probe: Some(Probe {
                r#type: ProbeType::Tcp { host: None },
                port: 443,
                initial_delay_seconds: 30,
                timeout_seconds: 5,
                period_seconds: 10,
                success_threshold: 1,
                failure_threshold: 50,
            }),
            liveness_probe: Some(Probe {
                r#type: ProbeType::Tcp { host: None },
                port: 443,
                initial_delay_seconds: 30,
                timeout_seconds: 5,
                period_seconds: 10,
                success_threshold: 1,
                failure_threshold: 50,
            }),
            storages: vec![],
            environment_vars_with_infos: btreemap! { "MY_VAR".to_string() => VariableInfo{ value: general_purpose::STANDARD.encode("my_value"), is_secret:false} },
            mounted_files: vec![],
            advanced_settings: Default::default(),
            annotations_group_ids: BTreeSet::new(),
            labels_group_ids: btreeset! {},
        }];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok());

        // check we can connect on ports
        sleep(Duration::from_secs(30));
        let now = Instant::now();
        let timeout = Duration::from_secs(60 * 10);
        loop {
            if now.elapsed() > timeout {
                panic!("Cannot connect to endpoint before timeout of {timeout:?}");
            }

            sleep(Duration::from_secs(10));

            // check we can connect on port
            let domain = format!("p443-{}.{}:443", service_id, infra_ctx.dns_provider().domain());
            let domain2 = format!("p5432-{}.{}:5432", service_id, infra_ctx.dns_provider().domain());
            if std::net::TcpStream::connect(domain).is_err() {
                continue;
            }

            if std::net::TcpStream::connect(domain2).is_err() {
                continue;
            }

            // exit loop
            break;
        }

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        "".to_string()
    })
}

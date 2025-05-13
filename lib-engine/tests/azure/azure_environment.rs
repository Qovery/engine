use crate::helpers;
use crate::helpers::azure::{azure_infra_config, clean_environments};
use crate::helpers::common::Infrastructure;
use crate::helpers::kubernetes::TargetCluster;
use crate::helpers::utilities::{
    FuncTestsSecrets, context_for_resource, engine_run_test, get_pods, init, logger, metrics_registry,
};
use base64::Engine;
use base64::engine::general_purpose;
use bstr::ByteSlice;
use function_name::named;
use k8s_openapi::api::batch::v1::CronJob;
use kube::Api;
use kube::api::ListParams;
use qovery_engine::cmd::kubectl::kubectl_get_secret;
use qovery_engine::environment::models::azure::AzureStorageType;
use qovery_engine::infrastructure::models::cloud_provider::Kind;
use qovery_engine::infrastructure::models::cloud_provider::azure::locations::AzureLocation;
use qovery_engine::infrastructure::models::container_registry::InteractWithRegistry;
use qovery_engine::io_models::annotations_group::{Annotation, AnnotationsGroup, AnnotationsGroupScope};
use qovery_engine::io_models::application::Protocol::HTTP;
use qovery_engine::io_models::application::{Port, Protocol, Storage};
use qovery_engine::io_models::container::{Container, Registry};
use qovery_engine::io_models::context::CloneForTest;
use qovery_engine::io_models::job::{ContainerRegistries, Job, JobSchedule, JobSource, LifecycleType};
use qovery_engine::io_models::labels_group::{Label, LabelsGroup};
use qovery_engine::io_models::probe::{Probe, ProbeType};
use qovery_engine::io_models::router::{CustomDomain, Route, Router};
use qovery_engine::io_models::terraform_service::{
    PersistentStorage, TerraformAction, TerraformActionCommand, TerraformBackend, TerraformBackendType,
    TerraformFilesSource, TerraformProvider, TerraformService,
};
use qovery_engine::io_models::variable_utils::VariableInfo;
use qovery_engine::io_models::{Action, MountedFile, QoveryIdentifier};
use qovery_engine::runtime::block_on;
use qovery_engine::utilities::to_short_id;
use std::collections::BTreeMap;
use std::str::FromStr;
use tracing::Level;
use tracing::log::warn;
use tracing::span;
use url::Url;
use uuid::Uuid;

#[cfg(feature = "test-azure-minimal")]
#[named]
#[test]
fn azure_test_build_phase() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AZURE_TEST_ORGANIZATION_LONG_ID
                .expect("AZURE_TEST_ORGANIZATION_LONG_ID  should be set"),
            secrets
                .AZURE_TEST_CLUSTER_LONG_ID
                .expect("AZURE_TEST_CLUSTER_LONG_ID  should be set"),
        );
        let region = AzureLocation::from_str(
            secrets
                .AZURE_DEFAULT_REGION
                .as_ref()
                .expect("AZURE_DEFAULT_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown Azure region");
        let target_cluster_azure_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .AZURE_TEST_KUBECONFIG_b64
                .expect("AZURE_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };

        let infra_ctx =
            azure_infra_config(&target_cluster_azure_test, &context, logger.clone(), metrics_registry.clone());
        let environment = helpers::environment::working_minimal_environment(&context);

        let env_action = environment.clone();

        let (env, ret) = environment.build_environment(&env_action, &infra_ctx);
        assert!(ret.is_ok());

        // Check the image exist in the registry
        let img_exist = infra_ctx
            .container_registry()
            .image_exists(&env.applications[0].get_build().image);
        assert!(img_exist);

        if let Err(e) = clean_environments(&context, vec![environment], region) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-azure-self-hosted")]
#[named]
#[test]
fn azure_aks_deploy_a_working_environment_without_router() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AZURE_TEST_ORGANIZATION_LONG_ID
                .expect("AZURE_TEST_ORGANIZATION_LONG_ID"),
            secrets.AZURE_TEST_CLUSTER_LONG_ID.expect("AZURE_TEST_CLUSTER_LONG_ID"),
        );
        let region = AzureLocation::from_str(
            secrets
                .AZURE_DEFAULT_REGION
                .as_ref()
                .expect("AZURE_DEFAULT_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown Azure region");
        let target_cluster_azure_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .AZURE_TEST_KUBECONFIG_b64
                .expect("AZURE_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx =
            azure_infra_config(&target_cluster_azure_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = azure_infra_config(
            &target_cluster_azure_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
        );
        let annotations_group_id = Uuid::new_v4();
        let mut environment = helpers::environment::working_minimal_environment(&context);
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

#[cfg(feature = "test-azure-self-hosted")]
#[named]
#[test]
fn azure_aks_deploy_a_working_environment_with_shared_registry() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry_for_deployment = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AZURE_TEST_ORGANIZATION_LONG_ID
                .expect("AZURE_TEST_ORGANIZATION_LONG_ID"),
            secrets.AZURE_TEST_CLUSTER_LONG_ID.expect("AZURE_TEST_CLUSTER_LONG_ID"),
        );
        let region = AzureLocation::from_str(
            secrets
                .AZURE_DEFAULT_REGION
                .as_ref()
                .expect("AZURE_DEFAULT_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown Azure region");
        let target_cluster_azure_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .AZURE_TEST_KUBECONFIG_b64
                .expect("AZURE_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };

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

        let infra_ctx = azure_infra_config(
            &target_cluster_azure_test,
            &context,
            logger.clone(),
            metrics_registry_for_deployment.clone(),
        );
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = azure_infra_config(
            &target_cluster_azure_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
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

        // TODO(QOV-762): this check is deactivated since we do not delete here here
        // let img_exist = infra_ctx
        //     .container_registry()
        //     .image_exists(&env.applications[0].get_build().image);
        // assert!(!img_exist);

        if let Err(e) = clean_environments(&context, vec![environment], region) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-azure-self-hosted")]
#[named]
#[test]
fn azure_aks_deploy_a_not_working_environment_without_router() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name,);
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AZURE_TEST_ORGANIZATION_LONG_ID
                .expect("AZURE_TEST_ORGANIZATION_LONG_ID"),
            secrets.AZURE_TEST_CLUSTER_LONG_ID.expect("AZURE_TEST_CLUSTER_LONG_ID"),
        );
        let region = AzureLocation::from_str(
            secrets
                .AZURE_DEFAULT_REGION
                .as_ref()
                .expect("AZURE_DEFAULT_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown Azure region");
        let target_cluster_azure_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .AZURE_TEST_KUBECONFIG_b64
                .expect("AZURE_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx =
            azure_infra_config(&target_cluster_azure_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = azure_infra_config(
            &target_cluster_azure_test,
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

#[cfg(feature = "test-azure-self-hosted")]
#[named]
#[test]
fn azure_aks_deploy_a_working_environment_and_pause() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AZURE_TEST_ORGANIZATION_LONG_ID
                .expect("AZURE_TEST_ORGANIZATION_LONG_ID"),
            secrets.AZURE_TEST_CLUSTER_LONG_ID.expect("AZURE_TEST_CLUSTER_LONG_ID"),
        );
        let region = AzureLocation::from_str(
            secrets
                .AZURE_DEFAULT_REGION
                .as_ref()
                .expect("AZURE_DEFAULT_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown Azure region");
        let target_cluster_azure_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .AZURE_TEST_KUBECONFIG_b64
                .expect("AZURE_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx =
            azure_infra_config(&target_cluster_azure_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = azure_infra_config(
            &target_cluster_azure_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
        );
        let environment = helpers::environment::working_minimal_environment(&context);

        let env_action = environment.clone();
        let selector = &environment.applications[0].long_id;

        let result = environment.deploy_environment(&env_action, &infra_ctx);
        assert!(result.is_ok());

        let ret = get_pods(&infra_ctx, Kind::Azure, &environment, selector);
        assert!(ret.is_ok());
        assert!(!ret.unwrap().items.is_empty());

        let result = environment.pause_environment(&env_action, &infra_ctx_for_delete);
        assert!(result.is_ok());

        // Check that we have actually 0 pods running for this app
        let ret = get_pods(&infra_ctx, Kind::Azure, &environment, selector);
        assert!(ret.is_ok());
        assert!(ret.unwrap().items.is_empty());

        // Check we can resume the env
        let ctx_resume = context.clone_not_same_execution_id();
        let infra_ctx_resume = azure_infra_config(
            &target_cluster_azure_test,
            &ctx_resume,
            logger.clone(),
            metrics_registry.clone(),
        );
        let result = environment.deploy_environment(&env_action, &infra_ctx_resume);
        assert!(result.is_ok());

        let ret = get_pods(&infra_ctx, Kind::Gcp, &environment, selector);
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

#[cfg(feature = "test-azure-self-hosted")]
#[named]
#[test]
fn azure_aks_deploy_a_working_environment_with_domain() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test",);
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AZURE_TEST_ORGANIZATION_LONG_ID
                .expect("AZURE_TEST_ORGANIZATION_LONG_ID"),
            secrets.AZURE_TEST_CLUSTER_LONG_ID.expect("AZURE_TEST_CLUSTER_LONG_ID"),
        );
        let region = AzureLocation::from_str(
            secrets
                .AZURE_DEFAULT_REGION
                .as_ref()
                .expect("AZURE_DEFAULT_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown Azure region");
        let target_cluster_azure_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .AZURE_TEST_KUBECONFIG_b64
                .expect("AZURE_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx =
            azure_infra_config(&target_cluster_azure_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = azure_infra_config(
            &target_cluster_azure_test,
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
                use_cdn: true,
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

        if let Err(e) = clean_environments(&context, vec![environment], region) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-azure-minimal")]
#[named]
#[test]
fn azure_aks_deploy_container_with_router() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AZURE_TEST_ORGANIZATION_LONG_ID
                .expect("AZURE_TEST_ORGANIZATION_LONG_ID"),
            secrets.AZURE_TEST_CLUSTER_LONG_ID.expect("AZURE_TEST_CLUSTER_LONG_ID"),
        );
        let region = AzureLocation::from_str(
            secrets
                .AZURE_DEFAULT_REGION
                .as_ref()
                .expect("AZURE_DEFAULT_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown Azure region");
        let target_cluster_azure_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .AZURE_TEST_KUBECONFIG_b64
                .expect("AZURE_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx =
            azure_infra_config(&target_cluster_azure_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = azure_infra_config(
            &target_cluster_azure_test,
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

        let annotations_group_id = Uuid::new_v4();
        let labels_group_id = Uuid::new_v4();
        environment.applications = vec![];
        let service_id = Uuid::new_v4();
        environment.containers = vec![Container {
            long_id: service_id,
            name: "👾👾👾 my little container 澳大利亚和智利提及年度采购计划 👾👾👾".to_string(),
            kube_name: format!("my-little-container-{}", suffix),
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
                    is_default: true,
                    name: format!("http-{}", suffix),
                    publicly_accessible: true,
                    protocol: HTTP,
                    service_name: None,
                    namespace: None,
                    additional_service: None,
                },
                Port {
                    long_id: Uuid::new_v4(),
                    port: 8081,
                    is_default: false,
                    name: format!("grpc-{}", suffix),
                    publicly_accessible: false,
                    protocol: HTTP,
                    service_name: None,
                    namespace: None,
                    additional_service: None,
                },
            ],
            readiness_probe: Some(Probe {
                r#type: ProbeType::Http {
                    path: "/".to_string(),
                    scheme: "HTTP".to_string(),
                },
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
            storages: vec![],
            environment_vars_with_infos: btreemap! { "MY_VAR".to_string() => VariableInfo{value: general_purpose::STANDARD.encode("my_value"), is_secret:false} },
            mounted_files: vec![],
            advanced_settings: Default::default(),
            annotations_group_ids: btreeset! { annotations_group_id },
            labels_group_ids: btreeset! { labels_group_id },
        }];
        environment.annotations_groups = btreemap! { annotations_group_id => AnnotationsGroup {
            annotations: vec![Annotation {
                key: "annot_key".to_string(),
                value: "annot_value".to_string(),
            },
            Annotation {
                key: "annot_key2".to_string(),
                value: "false".to_string(),
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
                propagate_to_cloud_provider: true,
            }]
        }};

        environment.routers = vec![Router {
            long_id: Uuid::new_v4(),
            name: "default-router".to_string(),
            kube_name: format!("router-{}", suffix),
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

        if let Err(e) = clean_environments(&context, vec![environment], region) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-azure-minimal")]
#[named]
#[test]
fn azure_aks_deploy_container_with_storages() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AZURE_TEST_ORGANIZATION_LONG_ID
                .expect("AZURE_TEST_ORGANIZATION_LONG_ID"),
            secrets.AZURE_TEST_CLUSTER_LONG_ID.expect("AZURE_TEST_CLUSTER_LONG_ID"),
        );
        let region = AzureLocation::from_str(
            secrets
                .AZURE_DEFAULT_REGION
                .as_ref()
                .expect("AZURE_DEFAULT_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown Azure region");
        let target_cluster_azure_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .AZURE_TEST_KUBECONFIG_b64
                .expect("AZURE_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = azure_infra_config(&target_cluster_azure_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();

        let infra_ctx_for_delete = azure_infra_config(
            &target_cluster_azure_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        let mut environment = helpers::environment::working_minimal_environment(&context);

        environment.applications = vec![];

        let storage_id_1 = Uuid::new_v4();
        let storage_id_2 = Uuid::new_v4();
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
                socat TCP-LISTEN:8080,bind=0.0.0.0,reuseaddr,fork STDOUT
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
            ports: vec![Port {
                long_id: Uuid::new_v4(),
                port: 8080,
                is_default: true,
                name: "http".to_string(),
                publicly_accessible: false,
                protocol: HTTP,
                service_name: None,
                namespace: None,
                additional_service: None,
            }],
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
            storages: vec![
                Storage {
                    id: to_short_id(&storage_id_1),
                    long_id: storage_id_1,
                    name: "photos1".to_string(),
                    storage_class: AzureStorageType::StandardSSDZRS.to_k8s_storage_class(),
                    size_in_gib: 10,
                    mount_point: "/mnt/photos1".to_string(),
                    snapshot_retention_in_days: 0,
                },
                Storage {
                    id: to_short_id(&storage_id_2),
                    long_id: storage_id_2,
                    name: "photos2".to_string(),
                    storage_class: AzureStorageType::StandardSSDZRS.to_k8s_storage_class(),
                    size_in_gib: 10,
                    mount_point: "/mnt/photos2".to_string(),
                    snapshot_retention_in_days: 0,
                },
            ],
            environment_vars_with_infos: BTreeMap::default(),
            mounted_files: vec![],
            advanced_settings: Default::default(),
            annotations_group_ids: btreeset! {},
            labels_group_ids: btreeset! {},
        }];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok());

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        if let Err(e) = clean_environments(&context, vec![environment], region) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-azure-self-hosted")]
#[named]
#[test]
fn azure_aks_deploy_container_with_mounted_files_as_volume() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AZURE_TEST_ORGANIZATION_LONG_ID
                .expect("AZURE_TEST_ORGANIZATION_LONG_ID"),
            secrets.AZURE_TEST_CLUSTER_LONG_ID.expect("AZURE_TEST_CLUSTER_LONG_ID"),
        );
        let region = AzureLocation::from_str(
            secrets
                .AZURE_DEFAULT_REGION
                .as_ref()
                .expect("AZURE_DEFAULT_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown Azure region");
        let target_cluster_azure_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .AZURE_TEST_KUBECONFIG_b64
                .expect("AZURE_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = azure_infra_config(&target_cluster_azure_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = azure_infra_config(
            &target_cluster_azure_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        let mut environment = helpers::environment::working_minimal_environment(&context);

        let mounted_file_identifier = QoveryIdentifier::new_random();
        let mounted_file = MountedFile {
            id: mounted_file_identifier.short().to_string(),
            long_id: mounted_file_identifier.to_uuid(),
            mount_path: "/this-file-should-exist".to_string(),
            file_content_b64: general_purpose::STANDARD.encode("I exist !"),
        };

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
                    is_default: true,
                    name: "http".to_string(),
                    publicly_accessible: true,
                    protocol: HTTP,
                    service_name: None,
                    namespace: None,
                    additional_service: None,
                },
                Port {
                    long_id: Uuid::new_v4(),
                    port: 8081,
                    is_default: false,
                    name: "grpc".to_string(),
                    publicly_accessible: false,
                    protocol: HTTP,
                    service_name: None,
                    namespace: None,
                    additional_service: None,
                },
            ],
            readiness_probe: Some(Probe {
                r#type: ProbeType::Tcp { host: None },
                port: 8080,
                initial_delay_seconds: 10,
                timeout_seconds: 2,
                period_seconds: 3,
                success_threshold: 1,
                failure_threshold: 5,
            }),
            liveness_probe: Some(Probe {
                r#type: ProbeType::Tcp { host: None },
                port: 8080,
                initial_delay_seconds: 10,
                timeout_seconds: 2,
                period_seconds: 3,
                success_threshold: 1,
                failure_threshold: 5,
            }),
            storages: vec![],
            environment_vars_with_infos: btreemap! { "MY_VAR".to_string() => VariableInfo{value: general_purpose::STANDARD.encode("my_value"), is_secret: false} },
            mounted_files: vec![mounted_file.clone()],
            advanced_settings: Default::default(),
            annotations_group_ids: btreeset! {},
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

        if let Err(e) = clean_environments(&context, vec![environment], region) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-azure-minimal")]
#[named]
#[test]
fn azure_aks_deploy_container_without_router() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AZURE_TEST_ORGANIZATION_LONG_ID
                .expect("AZURE_TEST_ORGANIZATION_LONG_ID"),
            secrets.AZURE_TEST_CLUSTER_LONG_ID.expect("AZURE_TEST_CLUSTER_LONG_ID"),
        );
        let region = AzureLocation::from_str(
            secrets
                .AZURE_DEFAULT_REGION
                .as_ref()
                .expect("AZURE_DEFAULT_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown Azure region");
        let target_cluster_azure_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .AZURE_TEST_KUBECONFIG_b64
                .expect("AZURE_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = azure_infra_config(&target_cluster_azure_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = azure_infra_config(
            &target_cluster_azure_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();

        let annotations_group_id = Uuid::new_v4();
        let labels_group_id = Uuid::new_v4();
        environment.applications = vec![];
        let service_id = Uuid::new_v4();
        environment.containers = vec![Container {
            long_id: service_id,
            name: "👾👾👾 my little container 澳大利亚和智利提及年度采购计划 👾👾👾".to_string(),
            kube_name: format!("my-little-container-{}", suffix),
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
                    is_default: true,
                    name: format!("http-{}", suffix),
                    publicly_accessible: true,
                    protocol: HTTP,
                    service_name: None,
                    namespace: None,
                    additional_service: None,
                },
                Port {
                    long_id: Uuid::new_v4(),
                    port: 8081,
                    is_default: false,
                    name: format!("grpc-{}", suffix),
                    publicly_accessible: false,
                    protocol: HTTP,
                    service_name: None,
                    namespace: None,
                    additional_service: None,
                },
            ],
            readiness_probe: Some(Probe {
                r#type: ProbeType::Http {
                    path: "/".to_string(),
                    scheme: "HTTP".to_string(),
                },
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
            storages: vec![],
            environment_vars_with_infos: btreemap! { "MY_VAR".to_string() => VariableInfo{value: general_purpose::STANDARD.encode("my_value"), is_secret:false} },
            mounted_files: vec![],
            advanced_settings: Default::default(),
            annotations_group_ids: btreeset! { annotations_group_id },
            labels_group_ids: btreeset! { labels_group_id },
        }];
        environment.annotations_groups = btreemap! { annotations_group_id => AnnotationsGroup {
            annotations: vec![Annotation {
                key: "annot_key".to_string(),
                value: "annot_value".to_string(),
            },
            Annotation {
                key: "annot_key2".to_string(),
                value: "false".to_string(),
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
                propagate_to_cloud_provider: true,
            }]
        }};

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok());

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        if let Err(e) = clean_environments(&context, vec![environment], region) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-azure-minimal")]
#[named]
#[test]
fn azure_aks_deploy_job() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", function_name!());
        let _enter = span.enter();

        let logger = logger();

        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AZURE_TEST_ORGANIZATION_LONG_ID
                .expect("AZURE_TEST_ORGANIZATION_LONG_ID"),
            secrets.AZURE_TEST_CLUSTER_LONG_ID.expect("AZURE_TEST_CLUSTER_LONG_ID"),
        );
        let region = AzureLocation::from_str(
            secrets
                .AZURE_DEFAULT_REGION
                .as_ref()
                .expect("AZURE_DEFAULT_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown Azure region");
        let target_cluster_azure_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .AZURE_TEST_KUBECONFIG_b64
                .expect("AZURE_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = azure_infra_config(&target_cluster_azure_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = azure_infra_config(
            &target_cluster_azure_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        let mut environment = helpers::environment::working_minimal_environment(&context);

        let json_output = r#"{"foo": {"value": 123, "sensitive": true}, "foo_2": {"value": "bar_2"}, "foo_3": {"value": "bar_3", "description": "bar_3"}}"#;
        let job_id = QoveryIdentifier::new_random();
        //environment.long_id = Uuid::default();
        //environment.project_long_id = Uuid::default();
        environment.applications = vec![];
        environment.jobs = vec![Job {
            long_id: job_id.to_uuid(), //Uuid::default(),
            name: format!("job-test-{}", job_id.short()),
            kube_name: format!("job-test-{}", job_id.short()),
            action: Action::Create,
            schedule: JobSchedule::OnStart {
                lifecycle_type: LifecycleType::TERRAFORM,
            },
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
            annotations_group_ids: btreeset! {},
            labels_group_ids: btreeset! {},
            should_delete_shared_registry: false,
            shared_image_feature_enabled: false,
        }];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok());

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        if let Err(e) = clean_environments(&context, vec![environment], region) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-azure-minimal")]
#[named]
#[test]
fn azure_aks_deploy_job_with_dockerfile_content() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", function_name!());
        let _enter = span.enter();

        let logger = logger();

        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AZURE_TEST_ORGANIZATION_LONG_ID
                .expect("AZURE_TEST_ORGANIZATION_LONG_ID"),
            secrets.AZURE_TEST_CLUSTER_LONG_ID.expect("AZURE_TEST_CLUSTER_LONG_ID"),
        );
        let region = AzureLocation::from_str(
            secrets
                .AZURE_DEFAULT_REGION
                .as_ref()
                .expect("AZURE_DEFAULT_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown Azure region");
        let target_cluster_azure_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .AZURE_TEST_KUBECONFIG_b64
                .expect("AZURE_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = azure_infra_config(&target_cluster_azure_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = azure_infra_config(
            &target_cluster_azure_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        let mut environment = helpers::environment::working_minimal_environment(&context);

        let json_output = r#"{"foo": {"value": 123, "sensitive": true}, "foo_2": {"value": "bar_2"}}"#;
        let job_id = QoveryIdentifier::new_random();
        //environment.long_id = Uuid::default();
        //environment.project_long_id = Uuid::default();
        environment.applications = vec![];
        environment.jobs = vec![Job {
            long_id: job_id.to_uuid(), //Uuid::default(),
            name: format!("job-test-{}", job_id.short()),
            kube_name: format!("job-test-{}", job_id.short()),
            action: Action::Create,
            schedule: JobSchedule::OnStart {
                lifecycle_type: LifecycleType::GENERIC,
            }, //JobSchedule::Cron("* * * * *".to_string()),
            source: JobSource::Docker {
                git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
                git_credentials: None,
                branch: "main".to_string(),
                commit_id: "168be6d16d8ade877f679ae752de5d095d95b8d0".to_string(),
                root_path: "/".to_string(),
                dockerfile_path: None,
                dockerfile_content: Some(
                    r#"
FROM debian:bookworm-slim
CMD ["/bin/sh", "-c", "echo hello"]
                    "#
                    .trim()
                    .to_string(),
                ),
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
        }];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok());

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        if let Err(e) = clean_environments(&context, vec![environment], region) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-azure-minimal")]
#[named]
#[test]
fn azure_aks_deploy_cronjob() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", function_name!());
        let _enter = span.enter();

        let logger = logger();

        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AZURE_TEST_ORGANIZATION_LONG_ID
                .expect("AZURE_TEST_ORGANIZATION_LONG_ID"),
            secrets.AZURE_TEST_CLUSTER_LONG_ID.expect("AZURE_TEST_CLUSTER_LONG_ID"),
        );
        let region = AzureLocation::from_str(
            secrets
                .AZURE_DEFAULT_REGION
                .as_ref()
                .expect("AZURE_DEFAULT_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown Azure region");
        let target_cluster_azure_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .AZURE_TEST_KUBECONFIG_b64
                .expect("AZURE_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = azure_infra_config(&target_cluster_azure_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = azure_infra_config(
            &target_cluster_azure_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        let mut environment = helpers::environment::working_minimal_environment(&context);

        let annotations_group_id = Uuid::new_v4();
        let labels_group_id = Uuid::new_v4();
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
            annotations_group_ids: btreeset! { annotations_group_id },
            labels_group_ids: btreeset! {},
            should_delete_shared_registry: false,
            shared_image_feature_enabled: false,
        }];
        environment.annotations_groups = btreemap! { annotations_group_id => AnnotationsGroup {
            annotations: vec![Annotation {
                key: "annot_key".to_string(),
                value: "annot_value".to_string(),
            }],
            scopes: vec![
                AnnotationsGroupScope::CronJobs,
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

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok());

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        if let Err(e) = clean_environments(&context, vec![environment], region) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-azure-self-hosted")]
#[named]
#[test]
fn azure_aks_deploy_cronjob_force_trigger() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", function_name!());
        let _enter = span.enter();

        let logger = logger();

        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AZURE_TEST_ORGANIZATION_LONG_ID
                .expect("AZURE_TEST_ORGANIZATION_LONG_ID"),
            secrets.AZURE_TEST_CLUSTER_LONG_ID.expect("AZURE_TEST_CLUSTER_LONG_ID"),
        );
        let region = AzureLocation::from_str(
            secrets
                .AZURE_DEFAULT_REGION
                .as_ref()
                .expect("AZURE_DEFAULT_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown Azure region");
        let target_cluster_azure_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .AZURE_TEST_KUBECONFIG_b64
                .expect("AZURE_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = azure_infra_config(&target_cluster_azure_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = azure_infra_config(
            &target_cluster_azure_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        let mut environment = helpers::environment::working_minimal_environment(&context);

        environment.applications = vec![];

        let cronjob_uuid = Uuid::new_v4();
        environment.jobs = vec![Job {
            long_id: cronjob_uuid,
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
        }];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok());

        // verify cronjob is well uninstalled
        let cronjob_namespace = format!(
            "{}-{}",
            to_short_id(&environment.project_long_id),
            to_short_id(&environment.long_id)
        );
        let cronjob_label = format!("qovery.com/service-id={cronjob_uuid}");

        let k8s_cronjob_api: Api<CronJob> = Api::namespaced(
            infra_ctx
                .mk_kube_client()
                .expect("should always contain kube_client")
                .client(),
            &cronjob_namespace,
        );
        let result_list_cronjobs = block_on(k8s_cronjob_api.list(&ListParams::default().labels(&cronjob_label)));
        match result_list_cronjobs {
            Ok(list) => {
                assert!(list.items.is_empty());
            }
            Err(kube_error) => {
                panic!("{kube_error}");
            }
        }

        // delete environment
        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        if let Err(e) = clean_environments(&context, vec![environment], region) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-azure-minimal")]
#[named]
#[test]
fn azure_aks_build_and_deploy_job() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", function_name!());
        let _enter = span.enter();

        let logger = logger();

        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AZURE_TEST_ORGANIZATION_LONG_ID
                .expect("AZURE_TEST_ORGANIZATION_LONG_ID"),
            secrets.AZURE_TEST_CLUSTER_LONG_ID.expect("AZURE_TEST_CLUSTER_LONG_ID"),
        );
        let region = AzureLocation::from_str(
            secrets
                .AZURE_DEFAULT_REGION
                .as_ref()
                .expect("AZURE_DEFAULT_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown Azure region");
        let target_cluster_azure_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .AZURE_TEST_KUBECONFIG_b64
                .expect("AZURE_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = azure_infra_config(&target_cluster_azure_test, &context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = azure_infra_config(
            &target_cluster_azure_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry(),
        );

        let mut environment = helpers::environment::working_minimal_environment(&context);

        let annotations_group_id = Uuid::new_v4();
        let labels_group_id = Uuid::new_v4();
        let json_output = r#"{"foo": {"value": "bar", "sensitive": true}, "foo_2": {"value": "bar_2"}}"#;
        environment.applications = vec![];
        environment.jobs = vec![Job {
            long_id: Uuid::new_v4(),
            name: "job test #####".to_string(),
            kube_name: "job-test".to_string(),
            action: Action::Create,
            schedule: JobSchedule::OnStart {
                lifecycle_type: LifecycleType::TERRAFORM,
            },
            source: JobSource::Docker {
                git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
                commit_id: "d22414a253db2bcf3acf91f85565d2dabe9211cc".to_string(),
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
            annotations_group_ids: btreeset! { annotations_group_id },
            labels_group_ids: btreeset! { labels_group_id },
            should_delete_shared_registry: false,
            shared_image_feature_enabled: false,
        }];
        environment.annotations_groups = btreemap! { annotations_group_id => AnnotationsGroup {
            annotations: vec![Annotation {
                key: "annot_key".to_string(),
                value: "annot_value".to_string(),
            },
            Annotation {
                key: "annot_key2".to_string(),
                value: "true".to_string(),
            }],
            scopes: vec![
                AnnotationsGroupScope::Jobs,
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

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok());

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        if let Err(e) = clean_environments(&context, vec![environment], region) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-azure-minimal")]
#[named]
#[test]
fn azure_aks_deploy_terraform_service() {
    fn build_terraform_service(
        service_id: Uuid,
        service_kube_name: &str,
        annotations_group_id: Uuid,
        labels_group_id: Uuid,
        terraform_action: TerraformAction,
    ) -> TerraformService {
        TerraformService {
            long_id: service_id,
            name: "terraform service test #####".to_string(),
            kube_name: service_kube_name.to_string(),
            action: Action::Create,
            cpu_request_in_milli: 100,
            cpu_limit_in_milli: 100,
            ram_request_in_mib: 256,
            ram_limit_in_mib: 256,
            persistent_storage: PersistentStorage {
                storage_class: AzureStorageType::StandardSSDZRS.to_k8s_storage_class(),
                size_in_gib: 1,
            },
            tf_files_source: TerraformFilesSource::Git {
                git_url: Url::parse("https://github.com/Qovery/terraform_service_engine_testing.git").expect(""),
                git_credentials: None,
                commit_id: "6692594dd31285e1b881f85cd504d934a579d7c5".to_string(),
                root_module_path: "/simple_terraform".to_string(),
            },
            tf_var_file_paths: vec!["tfvars/echo.tfvars".to_string()],
            tf_vars: vec![("command_argument".to_string(), "Mr Ripley".to_string())],
            provider: TerraformProvider::Terraform,
            provider_version: "1.9.7".to_string(),
            terraform_action,
            backend: TerraformBackend {
                backend_type: TerraformBackendType::Kubernetes,
                configs: vec![],
            },
            timeout_sec: 300,
            environment_vars_with_infos: Default::default(),
            advanced_settings: Default::default(),
            annotations_group_ids: btreeset! { annotations_group_id },
            labels_group_ids: btreeset! { labels_group_id },
            shared_image_feature_enabled: false,
        }
    }

    let test_name = function_name!();
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AZURE_TEST_ORGANIZATION_LONG_ID
                .expect("AZURE_TEST_ORGANIZATION_LONG_ID"),
            secrets.AZURE_TEST_CLUSTER_LONG_ID.expect("AZURE_TEST_CLUSTER_LONG_ID"),
        );
        let region = AzureLocation::from_str(
            secrets
                .AZURE_DEFAULT_REGION
                .as_ref()
                .expect("AZURE_DEFAULT_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown Azure region");
        let target_cluster_azure_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .AZURE_TEST_KUBECONFIG_b64
                .expect("AZURE_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx =
            azure_infra_config(&target_cluster_azure_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = azure_infra_config(
            &target_cluster_azure_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
        );

        let mut environment = helpers::environment::working_minimal_environment(&context);

        let suffix = QoveryIdentifier::new_random().short().to_string();

        let annotations_group_id = Uuid::new_v4();
        let labels_group_id = Uuid::new_v4();
        environment.applications = vec![];
        let service_id = Uuid::new_v4();
        let execution_id = Uuid::new_v4();
        let kube_name = format!("my-little-terraform-service-{}", suffix);
        environment.terraform_services = vec![build_terraform_service(
            service_id,
            &kube_name,
            annotations_group_id,
            labels_group_id,
            TerraformAction {
                command: TerraformActionCommand::PlanOnly,
                plan_execution_id: Some(execution_id.to_string()),
            },
        )];
        environment.annotations_groups = btreemap! { annotations_group_id => AnnotationsGroup {
            annotations: vec![Annotation {
                key: "annot_key".to_string(),
                value: "annot_value".to_string(),
            },
            Annotation {
                key: "annot_key2".to_string(),
                value: "true".to_string(),
            }],
            scopes: vec![
                AnnotationsGroupScope::Jobs,
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

        environment.annotations_groups = btreemap! { annotations_group_id => AnnotationsGroup {
            annotations: vec![Annotation {
                key: "annot_key".to_string(),
                value: "annot_value".to_string(),
            },
            Annotation {
                key: "annot_key2".to_string(),
                value: "false".to_string(),
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
                propagate_to_cloud_provider: true,
            }]
        }};

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok());

        environment.terraform_services = vec![build_terraform_service(
            service_id,
            &kube_name,
            annotations_group_id,
            labels_group_id,
            TerraformAction {
                command: TerraformActionCommand::ApplyFromPlan,
                plan_execution_id: Some(execution_id.to_string()),
            },
        )];

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(ret.is_ok());

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;
        environment_for_delete.terraform_services = vec![build_terraform_service(
            service_id,
            &kube_name,
            annotations_group_id,
            labels_group_id,
            TerraformAction {
                command: TerraformActionCommand::Destroy,
                plan_execution_id: None,
            },
        )];

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(ret.is_ok());

        if let Err(e) = clean_environments(&context, vec![environment], region) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

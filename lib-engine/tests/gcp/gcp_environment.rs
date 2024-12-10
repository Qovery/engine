use crate::helpers;
use crate::helpers::common::Infrastructure;
use crate::helpers::gcp::{clean_environments, gcp_infra_config};
use crate::helpers::kubernetes::TargetCluster;
use crate::helpers::utilities::{
    context_for_resource, engine_run_test, get_pods, init, logger, metrics_registry, FuncTestsSecrets,
};
use base64::engine::general_purpose;
use base64::Engine;
use function_name::named;
use qovery_engine::cloud_provider::gcp::locations::GcpRegion;
use qovery_engine::cloud_provider::Kind;
use qovery_engine::io_models::annotations_group::{Annotation, AnnotationsGroup, AnnotationsGroupScope};
use qovery_engine::io_models::application::Protocol::HTTP;
use qovery_engine::io_models::application::{Port, Protocol};
use qovery_engine::io_models::container::{Container, Registry};
use qovery_engine::io_models::context::CloneForTest;
use qovery_engine::io_models::labels_group::{Label, LabelsGroup};
use qovery_engine::io_models::probe::{Probe, ProbeType};
use qovery_engine::io_models::router::{CustomDomain, Route, Router};
use qovery_engine::io_models::variable_utils::VariableInfo;
use qovery_engine::io_models::{Action, QoveryIdentifier};
use std::str::FromStr;
use tracing::log::warn;
use tracing::span;
use tracing::Level;
use url::Url;
use uuid::Uuid;

#[cfg(feature = "test-gcp-minimal")]
#[named]
#[test]
fn gcp_test_build_phase() {
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
                .GCP_TEST_ORGANIZATION_LONG_ID
                .expect("GCP_TEST_ORGANIZATION_LONG_ID  should be set"),
            secrets
                .GCP_TEST_CLUSTER_LONG_ID
                .expect("GCP_TEST_CLUSTER_LONG_ID  should be set"),
        );
        let target_cluster_gcp_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .GCP_TEST_KUBECONFIG_b64
                .expect("GCP_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };

        let infra_ctx = gcp_infra_config(&target_cluster_gcp_test, &context, logger.clone(), metrics_registry.clone());
        let environment = helpers::environment::working_minimal_environment(&context);

        let env_action = environment.clone();

        let (env, ret) = environment.build_environment(&env_action, &infra_ctx);
        assert!(ret.is_ok());

        // Check the image exist in the registry
        let img_exist = infra_ctx
            .container_registry()
            .image_exists(&env.applications[0].get_build().image);
        assert!(img_exist);

        // TODO(benjaminch): add clean registry

        test_name.to_string()
    })
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn gcp_gke_deploy_a_working_environment_with_no_router() {
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
                .GCP_TEST_ORGANIZATION_LONG_ID
                .expect("GCP_TEST_ORGANIZATION_LONG_ID"),
            secrets.GCP_TEST_CLUSTER_LONG_ID.expect("GCP_TEST_CLUSTER_LONG_ID"),
        );
        let region = GcpRegion::from_str(
            secrets
                .GCP_DEFAULT_REGION
                .as_ref()
                .expect("GCP_DEFAULT_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown GCP region");
        let target_cluster_gcp_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .GCP_TEST_KUBECONFIG_b64
                .expect("GCP_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = gcp_infra_config(&target_cluster_gcp_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = gcp_infra_config(
            &target_cluster_gcp_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
        );
        let annotations_group_id = Uuid::new_v4();
        let mut environment = helpers::environment::working_minimal_environment(&context);
        // environment.applications.first().unwrap().annotations_group_ids = btreemap! { annotations_group_id };
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

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
#[ignore = "This test does not work on CI yet"]
fn deploy_a_working_environment_with_shared_registry() {
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
                .GCP_TEST_ORGANIZATION_LONG_ID
                .expect("GCP_TEST_ORGANIZATION_LONG_ID"),
            secrets.GCP_TEST_CLUSTER_LONG_ID.expect("GCP_TEST_CLUSTER_LONG_ID"),
        );
        let target_cluster_gcp_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .GCP_TEST_KUBECONFIG_b64
                .expect("GCP_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = gcp_infra_config(&target_cluster_gcp_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = gcp_infra_config(
            &target_cluster_gcp_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
        );
        let annotations_group_id = Uuid::new_v4();

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

        let mut environment =
            helpers::environment::working_minimal_environment_with_custom_git_url(&context, repo_url.as_str());
        let mut environment2 =
            helpers::environment::working_minimal_environment_with_custom_git_url(&context, repo_url.as_str());
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
        environment2.annotations_groups = btreemap! { annotations_group_id => AnnotationsGroup {
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
                    .unwrap_or_else(|_| println!("Cannot delete test repository `{}` after test", repository_name));
            });
        test_name.to_string()
    })
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn gcp_gke_deploy_a_not_working_environment_with_no_router() {
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
                .GCP_TEST_ORGANIZATION_LONG_ID
                .expect("GCP_TEST_ORGANIZATION_LONG_ID"),
            secrets.GCP_TEST_CLUSTER_LONG_ID.expect("GCP_TEST_CLUSTER_LONG_ID"),
        );
        let region = GcpRegion::from_str(
            secrets
                .GCP_DEFAULT_REGION
                .as_ref()
                .expect("GCP_DEFAULT_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown GCP region");
        let target_cluster_gcp_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .GCP_TEST_KUBECONFIG_b64
                .expect("GCP_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = gcp_infra_config(&target_cluster_gcp_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = gcp_infra_config(
            &target_cluster_gcp_test,
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

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn gcp_gke_deploy_a_working_environment_and_pause() {
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
                .GCP_TEST_ORGANIZATION_LONG_ID
                .expect("GCP_TEST_ORGANIZATION_LONG_ID"),
            secrets.GCP_TEST_CLUSTER_LONG_ID.expect("GCP_TEST_CLUSTER_LONG_ID"),
        );
        let region = GcpRegion::from_str(
            secrets
                .GCP_DEFAULT_REGION
                .as_ref()
                .expect("GCP_DEFAULT_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown GCP region");
        let target_cluster_gcp_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .GCP_TEST_KUBECONFIG_b64
                .expect("GCP_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = gcp_infra_config(&target_cluster_gcp_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = gcp_infra_config(
            &target_cluster_gcp_test,
            &context_for_delete,
            logger.clone(),
            metrics_registry.clone(),
        );
        let environment = helpers::environment::working_minimal_environment(&context);

        let env_action = environment.clone();
        let selector = &environment.applications[0].long_id;

        let result = environment.deploy_environment(&env_action, &infra_ctx);
        assert!(result.is_ok());

        let ret = get_pods(&infra_ctx, Kind::Gcp, &environment, selector);
        assert!(ret.is_ok());
        assert!(!ret.unwrap().items.is_empty());

        let result = environment.pause_environment(&env_action, &infra_ctx_for_delete);
        assert!(result.is_ok());

        // Check that we have actually 0 pods running for this app
        let ret = get_pods(&infra_ctx, Kind::Gcp, &environment, selector);
        assert!(ret.is_ok());
        assert!(ret.unwrap().items.is_empty());

        // Check we can resume the env
        let ctx_resume = context.clone_not_same_execution_id();
        let infra_ctx_resume =
            gcp_infra_config(&target_cluster_gcp_test, &ctx_resume, logger.clone(), metrics_registry.clone());
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

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn gcp_gke_deploy_a_working_environment_with_domain() {
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
                .GCP_TEST_ORGANIZATION_LONG_ID
                .expect("GCP_TEST_ORGANIZATION_LONG_ID"),
            secrets.GCP_TEST_CLUSTER_LONG_ID.expect("GCP_TEST_CLUSTER_LONG_ID"),
        );
        let target_cluster_gcp_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .GCP_TEST_KUBECONFIG_b64
                .expect("GCP_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = gcp_infra_config(&target_cluster_gcp_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = gcp_infra_config(
            &target_cluster_gcp_test,
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

        test_name.to_string()
    })
}

#[cfg(feature = "test-gcp-minimal")]
#[named]
#[test]
fn gcp_gke_deploy_container_with_router() {
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .GCP_TEST_ORGANIZATION_LONG_ID
                .expect("GCP_TEST_ORGANIZATION_LONG_ID"),
            secrets.GCP_TEST_CLUSTER_LONG_ID.expect("GCP_TEST_CLUSTER_LONG_ID"),
        );
        let target_cluster_gcp_test = TargetCluster::MutualizedTestCluster {
            kubeconfig: secrets
                .GCP_TEST_KUBECONFIG_b64
                .expect("GCP_TEST_KUBECONFIG_b64 is not set")
                .to_string(),
        };
        let infra_ctx = gcp_infra_config(&target_cluster_gcp_test, &context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = gcp_infra_config(
            &target_cluster_gcp_test,
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

        "".to_string()
    })
}

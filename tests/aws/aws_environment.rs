use crate::helpers;
use crate::helpers::aws::aws_default_infra_config;
use crate::helpers::common::Infrastructure;
use crate::helpers::environment::session_is_sticky;
use crate::helpers::utilities::{
    context_for_resource, engine_run_test, get_pods, get_pvc, init, is_pod_restarted_env, logger, metrics_registry,
    FuncTestsSecrets,
};
use ::function_name::named;
use bstr::ByteSlice;
use k8s_openapi::api::batch::v1::CronJob;
use kube::api::ListParams;
use kube::Api;
use qovery_engine::cloud_provider::Kind;
use qovery_engine::cmd::kubectl::kubectl_get_secret;
use qovery_engine::io_models::application::{Port, Protocol, Storage, StorageType};

use qovery_engine::io_models::container::{Container, Registry};
use qovery_engine::io_models::context::CloneForTest;
use qovery_engine::io_models::helm_chart::{HelmChart, HelmChartSource, HelmRawValues, HelmValueSource};
use qovery_engine::io_models::job::{Job, JobSchedule, JobSource};
use qovery_engine::io_models::probe::{Probe, ProbeType};
use qovery_engine::io_models::router::{CustomDomain, Route, Router};
use qovery_engine::io_models::variable_utils::VariableInfo;
use qovery_engine::io_models::{Action, MountedFile, QoveryIdentifier};
use qovery_engine::metrics_registry::{MetricsRegistry, StepLabel, StepName, StepStatus};
use qovery_engine::runtime::block_on;
use qovery_engine::transaction::TransactionResult;
use qovery_engine::utilities::to_short_id;
use reqwest::StatusCode;
use retry::delay::Fibonacci;
use std::borrow::BorrowMut;
use std::collections::BTreeMap;
use std::net::UdpSocket;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::{Duration, Instant};
use tracing::{span, Level};
use url::Url;
use uuid::Uuid;

#[cfg(feature = "test-aws-minimal")]
#[named]
#[test]
fn aws_test_build_phase() {
    // This test tries to run up to the build phase of the engine
    // basically building and pushing each applications
    let test_name = function_name!();
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let environment = helpers::environment::working_minimal_environment(&context);

        let ea = environment.clone();

        let (env, ret) = environment.build_environment(&ea, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        // Check the the image exist in the registry
        let img_exist = infra_ctx
            .container_registry()
            .does_image_exists(&env.applications[0].get_build().image);
        assert!(img_exist);

        test_name.to_string()
    })
}

#[ignore]
#[cfg(feature = "test-aws-minimal")]
#[named]
#[test]
fn aws_test_build_phase_with_git_lfs() {
    // This test tries to run up to the build phase of the engine
    // basically building and pushing each applications
    let test_name = function_name!();
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let environment = helpers::environment::working_minimal_environment(&context);

        let mut ea = environment.clone();

        ea.applications[0].git_url = "https://github.com/qovery/engine-testing-lfs".to_string();
        ea.applications[0].commit_id = "1252dacc15a605c860e9f3c02e676daf02611011".to_string();
        let (env, ret) = environment.build_environment(&ea, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        // Check the the image exist in the registry
        let img_exist = infra_ctx
            .container_registry()
            .does_image_exists(&env.applications[0].get_build().image);
        assert!(img_exist);

        test_name.to_string()
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_a_working_environment_with_no_router_on_aws_eks() {
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
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry_for_deployment.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = aws_default_infra_config(&context_for_delete, logger.clone(), metrics_registry());

        let environment = helpers::environment::working_minimal_environment(&context);

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ea = environment.clone();
        let ea_delete = environment_for_delete.clone();

        let ret = environment.deploy_environment(&ea, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));
        let records = metrics_registry_for_deployment.get_records(environment.applications.first().unwrap().long_id);
        assert_eq!(records.len(), 5);

        let record_provision_repo = records
            .iter()
            .find(|step| step.step_name == StepName::RegistryCreateRepository)
            .unwrap();
        assert_eq!(record_provision_repo.step_name, StepName::RegistryCreateRepository);
        assert_eq!(record_provision_repo.label, StepLabel::Service);
        assert_eq!(record_provision_repo.id, environment.applications.first().unwrap().long_id);
        assert_eq!(record_provision_repo.status, Some(StepStatus::Success));
        assert!(record_provision_repo.duration.is_some());

        let record_git_clone = records
            .iter()
            .find(|step| step.step_name == StepName::GitClone)
            .unwrap();
        assert_eq!(record_git_clone.step_name, StepName::GitClone);
        assert_eq!(record_git_clone.label, StepLabel::Service);
        assert_eq!(record_git_clone.id, environment.applications.first().unwrap().long_id);
        assert_eq!(record_git_clone.status, Some(StepStatus::Success));
        assert!(record_git_clone.duration.is_some());

        let record_build = records.iter().find(|step| step.step_name == StepName::Build).unwrap();
        assert_eq!(record_build.step_name, StepName::Build);
        assert_eq!(record_build.label, StepLabel::Service);
        assert_eq!(record_build.id, environment.applications.first().unwrap().long_id);
        assert_eq!(record_build.status, Some(StepStatus::Success));
        assert!(record_build.duration.is_some());

        let record_deployment = records
            .iter()
            .find(|step| step.step_name == StepName::Deployment)
            .unwrap();
        assert_eq!(record_deployment.step_name, StepName::Deployment);
        assert_eq!(record_deployment.label, StepLabel::Service);
        assert_eq!(record_deployment.id, environment.applications.first().unwrap().long_id);
        assert_eq!(record_deployment.status, Some(StepStatus::Success));
        assert!(record_deployment.duration.is_some());

        let record_total = records.iter().find(|step| step.step_name == StepName::Total).unwrap();
        assert_eq!(record_total.step_name, StepName::Total);
        assert_eq!(record_total.label, StepLabel::Service);
        assert_eq!(record_total.id, environment.applications.first().unwrap().long_id);
        assert_eq!(record_total.status, Some(StepStatus::Success));
        assert!(record_deployment.duration.is_some());

        let records = metrics_registry_for_deployment.get_records(environment.long_id);
        assert_eq!(records.len(), 2);

        let record_total = records.iter().find(|step| step.step_name == StepName::Total).unwrap();
        assert_eq!(record_total.step_name, StepName::Total);
        assert_eq!(record_total.label, StepLabel::Environment);
        assert_eq!(record_total.id, environment.long_id);
        assert_eq!(record_total.status, Some(StepStatus::Success));
        assert!(record_total.duration.is_some());

        let record_provision = records
            .iter()
            .find(|step| step.step_name == StepName::ProvisionBuilder)
            .unwrap();
        assert_eq!(record_provision.step_name, StepName::ProvisionBuilder);
        assert_eq!(record_provision.label, StepLabel::Environment);
        assert_eq!(record_provision.id, environment.long_id);
        assert_eq!(record_provision.status, Some(StepStatus::Success));
        assert!(record_provision.duration.is_some());

        let ret = environment_for_delete.delete_environment(&ea_delete, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        test_name.to_string()
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_a_working_environment_and_pause_it_eks() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );

        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());

        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = aws_default_infra_config(&context_for_delete, logger.clone(), metrics_registry());
        let environment = helpers::environment::working_minimal_environment(&context);

        let ea = environment.clone();
        let srv_id = &environment.applications[0].long_id;

        let ret = environment.deploy_environment(&ea, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = get_pods(&infra_ctx, Kind::Aws, &environment.clone(), srv_id, secrets.clone());
        assert!(ret.is_ok());
        assert!(!ret.unwrap().items.is_empty());

        let ret = environment.pause_environment(&ea, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        // Check that we have actually 0 pods running for this app
        let ret = get_pods(&infra_ctx, Kind::Aws, &environment.clone(), srv_id, secrets.clone());
        assert!(ret.is_ok());
        assert!(ret.unwrap().items.is_empty());

        // Check we can resume the env
        let ctx_resume = context.clone_not_same_execution_id();
        let infra_ctx_resume = aws_default_infra_config(&ctx_resume, logger.clone(), metrics_registry());
        let ret = environment.deploy_environment(&ea, &infra_ctx_resume);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = get_pods(&infra_ctx, Kind::Aws, &environment.clone(), srv_id, secrets);
        assert!(ret.is_ok());
        assert!(!ret.unwrap().items.is_empty());

        // Cleanup
        let ret = environment.delete_environment(&ea, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        test_name.to_string()
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_a_not_working_environment_with_no_router_on_aws_eks() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = aws_default_infra_config(&context_for_delete, logger.clone(), metrics_registry());

        let mut environment = helpers::environment::non_working_environment(&context);
        environment.routers = vec![];

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;

        let ea = environment.clone();
        let ea_delete = environment_delete.clone();

        let ret = environment.deploy_environment(&ea, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Error(_)));

        let ret = environment_delete.delete_environment(&ea_delete, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok | TransactionResult::Error(_)));

        test_name.to_string()
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn build_with_buildpacks_and_deploy_a_working_environment() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let context_for_deletion = context.clone_not_same_execution_id();
        let infra_ctx_for_deletion =
            aws_default_infra_config(&context_for_deletion, logger.clone(), metrics_registry());
        let mut environment = helpers::environment::working_minimal_environment(&context);
        environment.applications = environment
            .applications
            .into_iter()
            .map(|mut app| {
                app.ports = vec![Port {
                    long_id: Default::default(),
                    port: 3000,
                    name: "p3000".to_string(),
                    is_default: true,
                    publicly_accessible: true,
                    protocol: Protocol::HTTP,
                }];
                app.commit_id = "f59237d603829636138e2f22a0549e33b5dd6e1f".to_string();
                app.branch = "simple-node-app".to_string();
                app.dockerfile_path = None;
                app.readiness_probe = Some(Probe {
                    r#type: ProbeType::Tcp { host: None },
                    port: 3000,
                    failure_threshold: 9,
                    success_threshold: 1,
                    initial_delay_seconds: 15,
                    period_seconds: 10,
                    timeout_seconds: 10,
                });
                app.liveness_probe = None;
                app
            })
            .collect::<Vec<qovery_engine::io_models::application::Application>>();

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;

        let ea = environment.clone();
        let ea_delete = environment_delete.clone();

        let ret = environment.deploy_environment(&ea, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = environment_delete.delete_environment(&ea_delete, &infra_ctx_for_deletion);
        assert!(matches!(ret, TransactionResult::Ok));

        test_name.to_string()
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn build_worker_with_buildpacks_and_deploy_a_working_environment() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let context_for_deletion = context.clone_not_same_execution_id();
        let infra_ctx_for_deletion =
            aws_default_infra_config(&context_for_deletion, logger.clone(), metrics_registry());
        let mut environment = helpers::environment::working_minimal_environment(&context);
        environment.applications = environment
            .applications
            .into_iter()
            .map(|mut app| {
                app.ports = vec![Port {
                    long_id: Default::default(),
                    port: 3000,
                    is_default: true,
                    name: "p3000".to_string(),
                    publicly_accessible: true,
                    protocol: Protocol::HTTP,
                }];
                app.commit_id = "f59237d603829636138e2f22a0549e33b5dd6e1f".to_string();
                app.branch = "simple-node-app".to_string();
                app.dockerfile_path = None;
                app.readiness_probe = Some(Probe {
                    r#type: ProbeType::Tcp { host: None },
                    port: 3000,
                    failure_threshold: 9,
                    success_threshold: 1,
                    initial_delay_seconds: 15,
                    period_seconds: 10,
                    timeout_seconds: 10,
                });
                app.liveness_probe = None;
                app
            })
            .collect::<Vec<qovery_engine::io_models::application::Application>>();

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;

        let ea = environment.clone();
        let ea_delete = environment_delete.clone();

        let ret = environment.deploy_environment(&ea, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = environment_delete.delete_environment(&ea_delete, &infra_ctx_for_deletion);
        assert!(matches!(ret, TransactionResult::Ok));

        test_name.to_string()
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_a_working_environment_with_domain() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let context_for_deletion = context.clone_not_same_execution_id();
        let infra_ctx_for_deletion =
            aws_default_infra_config(&context_for_deletion, logger.clone(), metrics_registry());
        let environment = helpers::environment::working_minimal_environment_with_router(
            &context,
            secrets
                .DEFAULT_TEST_DOMAIN
                .as_ref()
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;

        let ea = environment.clone();
        let ea_delete = environment_delete.clone();

        let ret = environment.deploy_environment(&ea, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = environment_delete.delete_environment(&ea_delete, &infra_ctx_for_deletion);
        assert!(matches!(ret, TransactionResult::Ok));

        test_name.to_string()
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_a_working_environment_with_custom_domain_and_disable_check_on_custom_domain() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let context_for_deletion = context.clone_not_same_execution_id();
        let infra_ctx_for_deletion =
            aws_default_infra_config(&context_for_deletion, logger.clone(), metrics_registry());
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
            };

            router.custom_domains = vec![cd];
            modified_environment.routers.push(router);
        }

        for mut application in environment.applications.into_iter() {
            let advanced_settings = application.advanced_settings.borrow_mut();
            application.ports.push(Port {
                long_id: Uuid::new_v4(),
                port: 5050,
                is_default: false,
                name: "grpc".to_string(),
                publicly_accessible: true,
                protocol: Protocol::GRPC,
            });
            // disable custom domain check
            advanced_settings.deployment_custom_domain_check_enabled = false;
            modified_environment.applications.push(application);
        }

        environment = modified_environment;

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;

        let ea = environment.clone();
        let ea_delete = environment_delete.clone();

        let ret = environment.deploy_environment(&ea, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = environment_delete.delete_environment(&ea_delete, &infra_ctx_for_deletion);
        assert!(matches!(ret, TransactionResult::Ok));

        test_name.to_string()
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_a_working_environment_with_storage_on_aws_eks() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let secrets = FuncTestsSecrets::new();
        let logger = logger();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let context_for_deletion = context.clone_not_same_execution_id();
        let infra_ctx_for_deletion =
            aws_default_infra_config(&context_for_deletion, logger.clone(), metrics_registry());

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
                    storage_type: StorageType::Ssd,
                    size_in_gib: storage_size,
                    mount_point: "/mnt/photos".to_string(),
                    snapshot_retention_in_days: 0,
                }];
                app
            })
            .collect::<Vec<qovery_engine::io_models::application::Application>>();

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;

        let ea = environment.clone();
        let ea_delete = environment_delete.clone();

        let ret = environment.deploy_environment(&ea, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        match get_pvc(&infra_ctx, Kind::Aws, &environment, secrets) {
            Ok(pvc) => assert_eq!(
                pvc.items.expect("No items in pvc")[0].spec.resources.requests.storage,
                format!("{storage_size}Gi")
            ),
            Err(_) => panic!(),
        };

        let ret = environment_delete.delete_environment(&ea_delete, &infra_ctx_for_deletion);
        assert!(matches!(ret, TransactionResult::Ok));

        test_name.to_string()
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_a_working_environment_with_mounted_files_as_volume() {
    // TODO(benjaminch): This test could be moved out of end to end tests as it doesn't require
    // any cloud provider to be performed (can run on local Kubernetes).

    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let secrets = FuncTestsSecrets::new();
        let logger = logger();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let context_for_deletion = context.clone_not_same_execution_id();
        let infra_ctx_for_deletion =
            aws_default_infra_config(&context_for_deletion, logger.clone(), metrics_registry());

        let mounted_file_identifier = QoveryIdentifier::new_random();
        let mounted_file = MountedFile {
            id: mounted_file_identifier.short().to_string(),
            long_id: mounted_file_identifier.to_uuid(),
            mount_path: "/this-file-should-exist".to_string(),
            file_content_b64: base64::encode("I exist !"),
        };

        let environment =
            helpers::environment::working_environment_with_application_and_stateful_crashing_if_file_doesnt_exist(
                &context,
                &mounted_file,
            );

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;

        let ea = environment.clone();
        let ea_delete = environment_delete.clone();

        let ret = environment.deploy_environment(&ea, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

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
            infra_ctx.kubernetes().kube_client().expect("kube client is not set"),
            format!("metadata.name={}-{}", &mounted_file.id, service_id).as_str(),
        )
        .expect("unable to find secret for selector");
        assert!(!config_maps.is_empty());
        for cm in config_maps {
            assert_eq!(
                base64::decode(&mounted_file.file_content_b64)
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
        assert!(matches!(ret, TransactionResult::Ok));

        test_name.to_string()
    })
}

// to check if app redeploy or not, it shouldn't
#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn redeploy_same_app_with_ebs() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let secrets = FuncTestsSecrets::new();
        let logger = logger();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let context_bis = context.clone_not_same_execution_id();
        let infra_ctx_bis = aws_default_infra_config(&context_bis, logger.clone(), metrics_registry());
        let context_for_deletion = context.clone_not_same_execution_id();
        let infra_ctx_for_deletion =
            aws_default_infra_config(&context_for_deletion, logger.clone(), metrics_registry());

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
                    storage_type: StorageType::Ssd,
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

        let ea = environment.clone();
        let ea2 = environment_redeploy.clone();
        let ea_delete = environment_delete.clone();

        let ret = environment.deploy_environment(&ea, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        match get_pvc(&infra_ctx, Kind::Aws, &environment, secrets.clone()) {
            Ok(pvc) => assert_eq!(
                pvc.items.expect("No items in pvc")[0].spec.resources.requests.storage,
                format!("{storage_size}Gi")
            ),
            Err(_) => panic!(),
        };

        let (_, number) = is_pod_restarted_env(
            &infra_ctx,
            Kind::Aws,
            &environment_check1,
            &environment_check1.applications[0].long_id,
            secrets.clone(),
        );

        let ret = environment_redeploy.deploy_environment(&ea2, &infra_ctx_bis);
        assert!(matches!(ret, TransactionResult::Ok));

        let (_, number2) = is_pod_restarted_env(
            &infra_ctx,
            Kind::Aws,
            &environment_check2,
            &environment_check2.applications[0].long_id,
            secrets,
        );
        //nothing change in the app, so, it shouldn't be restarted
        assert!(number.eq(&number2));
        let ret = environment_delete.delete_environment(&ea_delete, &infra_ctx_for_deletion);
        assert!(matches!(ret, TransactionResult::Ok));

        test_name.to_string()
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_a_not_working_environment_and_after_working_environment() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let context_for_not_working = context.clone_not_same_execution_id();
        let infra_ctx_for_not_working =
            aws_default_infra_config(&context_for_not_working, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = aws_default_infra_config(&context_for_delete, logger.clone(), metrics_registry());

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
                app.commit_id = "a6343aa14fb9f04ef4b68babf5bb9d4d21098cb2".to_string();
                app.environment_vars_with_infos = BTreeMap::default();
                app
            })
            .collect::<Vec<qovery_engine::io_models::application::Application>>();
        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // environment actions
        let ea = environment.clone();
        let ea_not_working = environment_for_not_working.clone();
        let ea_delete = environment_for_delete.clone();

        let ret = environment_for_not_working.deploy_environment(&ea_not_working, &infra_ctx_for_not_working);
        assert!(matches!(ret, TransactionResult::Error(_)));

        let ret = environment.deploy_environment(&ea, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = environment_for_delete.delete_environment(&ea_delete, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        test_name.to_string()
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[test]
#[ignore]
#[named]
#[allow(dead_code)] // todo: make it work and remove the next line
#[allow(unused_attributes)]
fn deploy_ok_fail_fail_ok_environment() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = "deploy_ok_fail_fail_ok_environment");
        let _enter = span.enter();

        // working env
        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let environment = helpers::environment::working_minimal_environment(&context);

        // not working 1
        let context_for_not_working_1 = context.clone_not_same_execution_id();
        let infra_ctx_for_not_working_1 =
            aws_default_infra_config(&context_for_not_working_1, logger.clone(), metrics_registry());
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
        let infra_ctx_for_not_working_2 =
            aws_default_infra_config(&context_for_not_working_2, logger.clone(), metrics_registry());
        let not_working_env_2 = not_working_env_1.clone();

        // work for delete
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = aws_default_infra_config(&context_for_delete, logger.clone(), metrics_registry());
        let mut delete_env = environment.clone();
        delete_env.action = Action::Delete;

        let ea = environment.clone();
        let ea_not_working_1 = not_working_env_1.clone();
        let ea_not_working_2 = not_working_env_2.clone();
        let ea_delete = delete_env.clone();

        // OK
        let ret = environment.deploy_environment(&ea, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        // FAIL and rollback
        let ret = not_working_env_1.deploy_environment(&ea_not_working_1, &infra_ctx_for_not_working_1);
        assert!(matches!(ret, TransactionResult::Error(_)));

        // FAIL and Rollback again
        let ret = not_working_env_2.deploy_environment(&ea_not_working_2, &infra_ctx_for_not_working_2);
        assert!(matches!(ret, TransactionResult::Error(_)));

        // Should be working
        let ret = environment.deploy_environment(&ea, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = delete_env.delete_environment(&ea_delete, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        test_name.to_string()
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_a_non_working_environment_with_no_failover_on_aws_eks() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let secrets = FuncTestsSecrets::new();
        let logger = logger();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let environment = helpers::environment::non_working_environment(&context);

        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = aws_default_infra_config(&context_for_delete, logger.clone(), metrics_registry());
        let mut delete_env = environment.clone();
        delete_env.action = Action::Delete;

        let ea = environment.clone();
        let ea_delete = delete_env.clone();

        let ret = environment.deploy_environment(&ea, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Error(_)));

        let ret = delete_env.delete_environment(&ea_delete, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        test_name.to_string()
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn aws_eks_deploy_a_working_environment_with_sticky_session() {
    use qovery_engine::models::router::RouterAdvancedSettings;

    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = aws_default_infra_config(&context_for_delete, logger.clone(), metrics_registry());
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

        let ret = environment.deploy_environment(&env_action, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        // checking cookie is properly set on the app
        let kubeconfig = infra_ctx.kubernetes().get_kubeconfig_file_path();
        assert!(kubeconfig.is_ok());
        let router = environment
            .routers
            .first()
            .unwrap()
            .to_router_domain(
                infra_ctx.context(),
                RouterAdvancedSettings::default(),
                infra_ctx.cloud_provider(),
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
                kubeconfig.as_ref().unwrap().as_str(),
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

        let ret = environment_for_delete.delete_environment(&env_action_for_delete, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        test_name.to_string()
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn aws_eks_deploy_a_working_environment_with_ip_whitelist_allowing_all() {
    use qovery_engine::models::router::RouterAdvancedSettings;

    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = aws_default_infra_config(&context_for_delete, logger.clone(), metrics_registry());
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
        assert!(matches!(result, TransactionResult::Ok));

        let kubeconfig = infra_ctx.kubernetes().get_kubeconfig_file_path();
        assert!(kubeconfig.is_ok());
        let router = whitelist_all_environment
            .routers
            .first()
            .unwrap()
            .to_router_domain(
                infra_ctx.context(),
                RouterAdvancedSettings::new(true, None, None, None),
                infra_ctx.cloud_provider(),
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
                kubeconfig.as_ref().unwrap().as_str(),
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
        assert!(matches!(result, TransactionResult::Ok));

        let ret = env_action_for_delete.delete_environment(&env_action_for_delete, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        test_name.to_string()
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn aws_eks_deploy_a_working_environment_with_ip_whitelist_deny_all() {
    use qovery_engine::models::router::RouterAdvancedSettings;

    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = aws_default_infra_config(&context_for_delete, logger.clone(), metrics_registry());
        let whitelist_all_environment =
            helpers::environment::environment_only_http_server_router_with_ip_whitelist_source_range(
                &context,
                secrets
                    .DEFAULT_TEST_DOMAIN
                    .as_ref()
                    .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                    .as_str(),
                Some("0.0.0.0/32".to_string()), // <- Allow all IPs
            );

        let mut whitelist_all_environment_for_delete = whitelist_all_environment.clone();
        whitelist_all_environment_for_delete.action = Action::Delete;

        let env_action = whitelist_all_environment.clone();
        let env_action_for_delete = whitelist_all_environment_for_delete.clone();

        let result = whitelist_all_environment.deploy_environment(&env_action, &infra_ctx);
        assert!(matches!(result, TransactionResult::Ok));

        let kubeconfig = infra_ctx.kubernetes().get_kubeconfig_file_path();
        assert!(kubeconfig.is_ok());
        let router = whitelist_all_environment
            .routers
            .first()
            .unwrap()
            .to_router_domain(
                infra_ctx.context(),
                RouterAdvancedSettings::new(true, None, None, None),
                infra_ctx.cloud_provider(),
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
                kubeconfig.as_ref().unwrap().as_str(),
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

        let result =
            whitelist_all_environment_for_delete.delete_environment(&env_action_for_delete, &infra_ctx_for_delete);
        assert!(matches!(result, TransactionResult::Ok));

        let ret = env_action_for_delete.delete_environment(&env_action_for_delete, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        test_name.to_string()
    })
}

#[cfg(feature = "test-aws-minimal")]
#[named]
#[test]
fn deploy_container_with_no_router_and_affinitiy_on_aws_eks() {
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = aws_default_infra_config(&context_for_delete, logger.clone(), metrics_registry());

        let mut environment = helpers::environment::working_minimal_environment(&context);

        let service_id = Uuid::new_v4();
        environment.applications = vec![];
        environment.containers = vec![Container {
            long_id: service_id,
            name: "👾👾👾 my little container 澳大利亚和智利提及年度采购计划 👾👾👾".to_string(),
            kube_name: "affinity-container".to_string(),
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
            cpu_request_in_mili: 250,
            cpu_limit_in_mili: 250,
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
                    name: "p8080".to_string(),
                    publicly_accessible: true,
                    protocol: Protocol::HTTP,
                },
                Port {
                    long_id: Uuid::new_v4(),
                    port: 8081,
                    is_default: false,
                    name: "grpc".to_string(),
                    publicly_accessible: false,
                    protocol: Protocol::HTTP,
                },
            ],
            storages: vec![],
            environment_vars_with_infos: btreemap! { "MY_VAR".to_string() => VariableInfo { value: base64::encode("my_value"), is_secret: false}},
            mounted_files: vec![],
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
            advanced_settings: Default::default(),
        }];

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        let kube_conn = infra_ctx.kubernetes().q_kube_client().expect("kube client is not set");
        // ensure default pod affinity is set to preferred
        let preferred = block_on(kube_conn.get_deployments_from_api(
            context.get_event_details(qovery_engine::events::Transmitter::Application(Uuid::new_v4(), "".to_string())),
            None,
            qovery_engine::services::kube_client::SelectK8sResourceBy::LabelsSelector(format!(
                "qovery.com/service-id={}",
                service_id
            )),
        ));
        assert!(preferred.is_ok());
        let deployments = preferred.unwrap().unwrap();
        for deploy in deployments {
            assert!(deploy
                .spec
                .unwrap()
                .template
                .spec
                .unwrap()
                .affinity
                .unwrap()
                .pod_anti_affinity
                .unwrap()
                .preferred_during_scheduling_ignored_during_execution
                .is_some())
        }

        // set node affinity and pod antiaffinity to required
        environment.containers[0].advanced_settings.deployment_antiaffinity_pod =
            qovery_engine::io_models::PodAntiAffinity::Required;
        let node_selector_key = "kubernetes.io/os";
        let node_selector_value = "linux";
        environment.containers[0]
            .advanced_settings
            .deployment_affinity_node_required = btreemap! {
            node_selector_key.to_string() => node_selector_value.to_string()
        };
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        let requirred = block_on(kube_conn.get_deployments_from_api(
            context.get_event_details(qovery_engine::events::Transmitter::Application(Uuid::new_v4(), "".to_string())),
            None,
            qovery_engine::services::kube_client::SelectK8sResourceBy::LabelsSelector(format!(
                "qovery.com/service-id={}",
                service_id
            )),
        ));
        assert!(requirred.is_ok());
        let deployments = requirred.unwrap().unwrap();
        for deploy in deployments {
            let pod_antiaffinity = deploy.spec.unwrap().template.spec.unwrap().affinity.unwrap();
            // check pod antiaffinity
            assert!(pod_antiaffinity
                .pod_anti_affinity
                .clone()
                .unwrap()
                .required_during_scheduling_ignored_during_execution
                .is_some());
            // check node selector
            let node_affinity = pod_antiaffinity
                .node_affinity
                .unwrap()
                .required_during_scheduling_ignored_during_execution
                .unwrap()
                .node_selector_terms[0]
                .clone()
                .match_expressions
                .unwrap();
            for nf in node_affinity {
                assert_eq!(nf.key, node_selector_key);
                assert_eq!(nf.values.unwrap()[0], node_selector_value);
            }
        }

        // delete
        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));
        "".to_string()
    })
}

#[cfg(feature = "test-aws-minimal")]
#[named]
#[test]
fn deploy_container_with_no_router_on_aws_eks() {
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry_for_deployment = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry_for_deployment.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let _infra_ctx_for_delete = aws_default_infra_config(&context_for_delete, logger.clone(), metrics_registry());

        let mut environment = helpers::environment::working_minimal_environment(&context);

        let service_id = Uuid::new_v4();
        environment.applications = vec![];
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
                socat TCP6-LISTEN:8080,bind=[::],reuseaddr,fork STDOUT
                "#
                .to_string(),
            ],
            entrypoint: None,
            cpu_request_in_mili: 250,
            cpu_limit_in_mili: 250,
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
                    name: "p8080".to_string(),
                    publicly_accessible: true,
                    protocol: Protocol::HTTP,
                },
                Port {
                    long_id: Uuid::new_v4(),
                    port: 8081,
                    is_default: false,
                    name: "grpc".to_string(),
                    publicly_accessible: false,
                    protocol: Protocol::HTTP,
                },
            ],
            storages: vec![],
            environment_vars_with_infos: btreemap! { "MY_VAR".to_string() => VariableInfo { value: base64::encode("my_value"), is_secret: false} },
            mounted_files: vec![],
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
            advanced_settings: Default::default(),
        }];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        let records = metrics_registry_for_deployment.get_records(environment.containers.first().unwrap().long_id);
        let mirror_record = records
            .iter()
            .find(|record| record.step_name == StepName::MirrorImage)
            .unwrap();
        assert_eq!(mirror_record.status, Some(StepStatus::Success));

        "".to_string()
    })
}

#[cfg(feature = "test-aws-minimal")]
#[named]
#[test]
fn deploy_container_with_storages_on_aws_eks() {
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = aws_default_infra_config(&context_for_delete, logger.clone(), metrics_registry());

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
                socat TCP6-LISTEN:8080,bind=[::],reuseaddr,fork STDOUT
                "#
                .to_string(),
            ],
            entrypoint: None,
            cpu_request_in_mili: 250,
            cpu_limit_in_mili: 250,
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
                protocol: Protocol::HTTP,
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
                    storage_type: StorageType::Ssd,
                    size_in_gib: 10,
                    mount_point: "/mnt/photos1".to_string(),
                    snapshot_retention_in_days: 0,
                },
                Storage {
                    id: to_short_id(&storage_id_2),
                    long_id: storage_id_2,
                    name: "photos2".to_string(),
                    storage_type: StorageType::Ssd,
                    size_in_gib: 10,
                    mount_point: "/mnt/photos2".to_string(),
                    snapshot_retention_in_days: 0,
                },
            ],
            environment_vars_with_infos: BTreeMap::default(),
            mounted_files: vec![],
            advanced_settings: Default::default(),
        }];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        "".to_string()
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_container_on_aws_eks_with_mounted_files_as_volume() {
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = aws_default_infra_config(&context_for_delete, logger.clone(), metrics_registry());

        let mut environment = helpers::environment::working_minimal_environment(&context);

        let mounted_file_identifier = QoveryIdentifier::new_random();
        let mounted_file = MountedFile {
            id: mounted_file_identifier.short().to_string(),
            long_id: mounted_file_identifier.to_uuid(),
            mount_path: "/this-file-should-exist".to_string(),
            file_content_b64: base64::encode("I exist !"),
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
            cpu_request_in_mili: 250,
            cpu_limit_in_mili: 250,
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
                    protocol: Protocol::HTTP,
                },
                Port {
                    long_id: Uuid::new_v4(),
                    port: 8081,
                    is_default: false,
                    name: "grpc".to_string(),
                    publicly_accessible: false,
                    protocol: Protocol::HTTP,
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
            environment_vars_with_infos: btreemap! { "MY_VAR".to_string() => VariableInfo{value: base64::encode("my_value"), is_secret: false} },
            mounted_files: vec![mounted_file.clone()],
            advanced_settings: Default::default(),
        }];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

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
            infra_ctx.kubernetes().kube_client().expect("kube client is not set"),
            format!("metadata.name={}-{}", &mounted_file.id, service_id).as_str(),
        )
        .expect("unable to find secret for selector");
        assert!(!config_maps.is_empty());
        for cm in config_maps {
            assert_eq!(
                base64::decode(&mounted_file.file_content_b64)
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
        assert!(matches!(ret, TransactionResult::Ok));

        "".to_string()
    })
}

#[cfg(feature = "test-aws-minimal")]
#[named]
#[test]
fn deploy_container_with_router_on_aws_eks() {
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = aws_default_infra_config(&context_for_delete, logger.clone(), metrics_registry());

        let mut environment = helpers::environment::working_minimal_environment(&context);

        environment.applications = vec![];
        let service_id = Uuid::new_v4();
        environment.containers = vec![Container {
            long_id: service_id,
            name: "👾👾👾 my little container 澳大利亚和智利提及年度采购计划 👾👾👾".to_string(),
            kube_name: "my-little-container".to_string(),
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
            cpu_request_in_mili: 250,
            cpu_limit_in_mili: 250,
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
                    name: "http".to_string(),
                    publicly_accessible: true,
                    protocol: Protocol::HTTP,
                },
                Port {
                    long_id: Uuid::new_v4(),
                    port: 8081,
                    is_default: false,
                    name: "grpc".to_string(),
                    publicly_accessible: false,
                    protocol: Protocol::HTTP,
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
            environment_vars_with_infos: btreemap! { "MY_VAR".to_string() => VariableInfo{value: base64::encode("my_value"), is_secret:false} },
            mounted_files: vec![],
            advanced_settings: Default::default(),
        }];

        environment.routers = vec![Router {
            long_id: Uuid::new_v4(),
            name: "default-router".to_string(),
            kube_name: "default-router".to_string(),
            action: Action::Create,
            default_domain: "main".to_string(),
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
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        "".to_string()
    })
}

#[cfg(feature = "test-aws-minimal")]
#[test]
fn deploy_job_on_aws_eks() {
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = "deploy_job_on_aws_eks");
        let _enter = span.enter();

        let logger = logger();

        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = aws_default_infra_config(&context_for_delete, logger.clone(), metrics_registry());

        let mut environment = helpers::environment::working_minimal_environment(&context);

        let json_output = r#"{"foo": {"value": 123, "sensitive": true}, "foo_2": {"value": "bar_2"}}"#;
        //environment.long_id = Uuid::default();
        //environment.project_long_id = Uuid::default();
        environment.applications = vec![];
        environment.jobs = vec![Job {
            long_id: Uuid::new_v4(), //Uuid::default(),
            name: "job test #####".to_string(),
            kube_name: "job-test".to_string(),
            action: Action::Create,
            schedule: JobSchedule::OnStart {}, //JobSchedule::Cron("* * * * *".to_string()),
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
        }];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        "".to_string()
    })
}

#[cfg(feature = "test-aws-minimal")]
#[test]
fn deploy_cronjob_on_aws_eks() {
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = "deploy_cronjob_on_aws_eks");
        let _enter = span.enter();

        let logger = logger();

        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = aws_default_infra_config(&context_for_delete, logger.clone(), metrics_registry());

        let mut environment = helpers::environment::working_minimal_environment(&context);

        environment.applications = vec![];
        environment.jobs = vec![Job {
            long_id: Uuid::new_v4(),
            name: "job test #####||||*_-(".to_string(),
            kube_name: "job-test".to_string(),
            action: Action::Create,
            schedule: JobSchedule::Cron {
                schedule: "* * * * *".to_string(),
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
        }];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        "".to_string()
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[test]
fn deploy_cronjob_force_trigger_on_aws_eks() {
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = "deploy_cronjob_on_aws_eks");
        let _enter = span.enter();

        let logger = logger();

        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = aws_default_infra_config(&context_for_delete, logger.clone(), metrics_registry());

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
        }];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        // verify cronjob is well uninstalled
        let cronjob_namespace = format!(
            "{}-{}",
            to_short_id(&environment.project_long_id),
            to_short_id(&environment.long_id)
        );
        let cronjob_label = format!("qovery.com/service-id={cronjob_uuid}");

        let k8s_cronjob_api: Api<CronJob> = Api::namespaced(
            infra_ctx
                .kubernetes()
                .kube_client()
                .expect("should always contain kube_client"),
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
        assert!(matches!(ret, TransactionResult::Ok));

        "".to_string()
    })
}

#[cfg(feature = "test-aws-minimal")]
#[test]
fn build_and_deploy_job_on_aws_eks() {
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = "build_and_deploy_job_on_aws_eks");
        let _enter = span.enter();

        let logger = logger();

        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = aws_default_infra_config(&context_for_delete, logger.clone(), metrics_registry());

        let mut environment = helpers::environment::working_minimal_environment(&context);

        let json_output = r#"{"foo": {"value": "bar", "sensitive": true}, "foo_2": {"value": "bar_2"}}"#;
        environment.applications = vec![];
        environment.jobs = vec![Job {
            long_id: Uuid::new_v4(),
            name: "job test #####".to_string(),
            kube_name: "job-test".to_string(),
            action: Action::Create,
            schedule: JobSchedule::OnStart {},
            source: JobSource::Docker {
                git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
                commit_id: "d22414a253db2bcf3acf91f85565d2dabe9211cc".to_string(),
                dockerfile_path: Some("Dockerfile".to_string()),
                root_path: String::from("/"),
                git_credentials: None,
                branch: "main".to_string(),
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
        }];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        "".to_string()
    })
}

#[cfg(feature = "test-aws-minimal")]
#[named]
#[test]
fn test_restart_deployment() {
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();

        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = aws_default_infra_config(&context_for_delete, logger.clone(), metrics_registry());

        let mut environment = helpers::environment::working_minimal_environment(&context);

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
                socat TCP6-LISTEN:8080,bind=[::],reuseaddr,fork STDOUT
                "#
                .to_string(),
            ],
            entrypoint: None,
            cpu_request_in_mili: 250,
            cpu_limit_in_mili: 250,
            ram_request_in_mib: 250,
            ram_limit_in_mib: 250,
            min_instances: 3,
            max_instances: 3,
            public_domain: format!("{}.{}", service_id, infra_ctx.dns_provider().domain()),
            ports: vec![
                Port {
                    long_id: Uuid::new_v4(),
                    port: 8080,
                    is_default: true,
                    name: "http".to_string(),
                    publicly_accessible: true,
                    protocol: Protocol::HTTP,
                },
                Port {
                    long_id: Uuid::new_v4(),
                    port: 8081,
                    is_default: false,
                    name: "grpc".to_string(),
                    publicly_accessible: false,
                    protocol: Protocol::HTTP,
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
            mounted_files: vec![],
            environment_vars_with_infos: btreemap! { "MY_VAR".to_string() => VariableInfo{value: base64::encode("my_value"), is_secret: false} },
            advanced_settings: Default::default(),
        }];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        sleep(Duration::from_secs(10));

        let result = environment.restart_environment(&environment, &infra_ctx);
        assert!(matches!(result, TransactionResult::Ok));

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        "".to_string()
    })
}

#[cfg(feature = "test-aws-minimal")]
#[named]
#[test]
fn test_restart_statefulset() {
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();

        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = aws_default_infra_config(&context_for_delete, logger.clone(), metrics_registry());

        let mut environment = helpers::environment::working_minimal_environment(&context);

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
                socat TCP6-LISTEN:8080,bind=[::],reuseaddr,fork STDOUT
                "#
                .to_string(),
            ],
            storages: vec![Storage {
                id: "z111111".to_string(),
                long_id: Uuid::new_v4(),
                name: "storage-1".to_string(),
                mount_point: "/storage".to_string(),
                size_in_gib: 10,
                storage_type: StorageType::FastSsd,
                snapshot_retention_in_days: 1,
            }],
            mounted_files: vec![],
            entrypoint: None,
            cpu_request_in_mili: 250,
            cpu_limit_in_mili: 250,
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
                    protocol: Protocol::HTTP,
                },
                Port {
                    long_id: Uuid::new_v4(),
                    port: 8081,
                    is_default: false,
                    name: "grpc".to_string(),
                    publicly_accessible: false,
                    protocol: Protocol::HTTP,
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
            environment_vars_with_infos: btreemap! { "MY_VAR".to_string() => VariableInfo{value: base64::encode("my_value"), is_secret: false} },
            advanced_settings: Default::default(),
        }];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        sleep(Duration::from_secs(10));

        let result = environment.restart_environment(&environment, &infra_ctx);
        assert!(matches!(result, TransactionResult::Ok));

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        "".to_string()
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[test]
fn build_and_deploy_job_on_aws_eks_with_mounted_files_as_volume() {
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = "build_and_deploy_job_on_aws_eks");
        let _enter = span.enter();

        let logger = logger();

        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = aws_default_infra_config(&context_for_delete, logger.clone(), metrics_registry());

        let mounted_file_identifier = QoveryIdentifier::new_random();
        let mounted_file = MountedFile {
            id: mounted_file_identifier.short().to_string(),
            long_id: mounted_file_identifier.to_uuid(),
            mount_path: "/this-file-should-exist.json".to_string(),
            file_content_b64: base64::encode(
                r#"{"foo": {"value": "bar", "sensitive": true}, "foo_2": {"value": "bar_2"}}"#,
            ),
        };

        let mut environment = helpers::environment::working_minimal_environment(&context);

        environment.applications = vec![];
        environment.jobs = vec![Job {
            long_id: Uuid::new_v4(),
            name: "job test #####".to_string(),
            kube_name: "job-test".to_string(),
            action: Action::Create,
            schedule: JobSchedule::OnStart {},
            source: JobSource::Docker {
                git_url: "https://github.com/Qovery/engine-testing.git".to_string(),
                commit_id: "d22414a253db2bcf3acf91f85565d2dabe9211cc".to_string(),
                dockerfile_path: Some("Dockerfile".to_string()),
                root_path: String::from("/"),
                git_credentials: None,
                branch: "main".to_string(),
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
        }];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

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
            infra_ctx.kubernetes().kube_client().expect("kube client is not set"),
            format!("metadata.name={}-{}", &mounted_file.id, service_id).as_str(),
        )
        .expect("unable to find secret for selector");
        assert!(!config_maps.is_empty());
        for cm in config_maps {
            assert_eq!(
                base64::decode(&mounted_file.file_content_b64)
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
        assert!(matches!(ret, TransactionResult::Ok));

        "".to_string()
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_a_working_environment_with_multiple_resized_storage_on_aws_eks() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let secrets = FuncTestsSecrets::new();
        let logger = logger();

        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let context_for_deletion = context.clone_not_same_execution_id();
        let infra_ctx_for_deletion =
            aws_default_infra_config(&context_for_deletion, logger.clone(), metrics_registry());

        let mut environment = helpers::environment::working_minimal_environment(&context);

        let initial_storage_size: u32 = 10;
        environment.applications = environment
            .applications
            .into_iter()
            .map(|mut app| {
                let id_1 = Uuid::new_v4();
                let id_2 = Uuid::new_v4();
                app.storage = vec![
                    Storage {
                        id: to_short_id(&id_1),
                        long_id: id_1,
                        name: "photos_1".to_string(),
                        storage_type: StorageType::Ssd,
                        size_in_gib: initial_storage_size,
                        mount_point: "/mnt/photos_1".to_string(),
                        snapshot_retention_in_days: 0,
                    },
                    Storage {
                        id: to_short_id(&id_2),
                        long_id: id_2,
                        name: "photos_2".to_string(),
                        storage_type: StorageType::Ssd,
                        size_in_gib: initial_storage_size,
                        mount_point: "/mnt/photos_2".to_string(),
                        snapshot_retention_in_days: 0,
                    },
                ];
                app
            })
            .collect::<Vec<qovery_engine::io_models::application::Application>>();

        let mut resized_environment = environment.clone();
        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;

        let ea = environment.clone();
        let ea_delete = environment_delete.clone();

        let ret = environment.deploy_environment(&ea, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        match get_pvc(&infra_ctx, Kind::Aws, &environment, secrets.clone()) {
            Ok(pvc) => {
                assert!(pvc.items.is_some());
                let pvcs = pvc.items.unwrap();
                let app_storages = &environment.applications[0].storage;
                assert_eq!(
                    pvcs.iter()
                        .find(|pvc| pvc.metadata.name.contains(&app_storages[0].id))
                        .expect("Unable to get storage 1")
                        .spec
                        .resources
                        .requests
                        .storage,
                    format!("{initial_storage_size}Gi")
                );
                assert_eq!(
                    pvcs.iter()
                        .find(|pvc| pvc.metadata.name.contains(&app_storages[1].id))
                        .expect("Unable to get storage 2")
                        .spec
                        .resources
                        .requests
                        .storage,
                    format!("{initial_storage_size}Gi")
                );
            }
            Err(_) => panic!(),
        };

        let resized_size = 20;
        resized_environment.applications[0].storage[0].size_in_gib = resized_size;
        let resized_ea = resized_environment.clone();
        let resized_context = context.clone_not_same_execution_id();
        let resized_infra_ctx = aws_default_infra_config(&resized_context, logger.clone(), metrics_registry());
        let resized_ret = resized_environment.deploy_environment(&resized_ea, &resized_infra_ctx);
        assert!(matches!(resized_ret, TransactionResult::Ok));

        match get_pvc(&resized_infra_ctx, Kind::Aws, &resized_environment, secrets) {
            Ok(pvc) => {
                assert!(pvc.items.is_some());
                let pvcs = pvc.items.unwrap();
                let app_storages = &resized_environment.applications[0].storage;
                assert_eq!(
                    pvcs.iter()
                        .find(|pvc| pvc.metadata.name.contains(&app_storages[0].id))
                        .expect("Unable to get storage 1")
                        .spec
                        .resources
                        .requests
                        .storage,
                    format!("{resized_size}Gi")
                );
                assert_eq!(
                    pvcs.iter()
                        .find(|pvc| pvc.metadata.name.contains(&app_storages[1].id))
                        .expect("Unable to get storage 2")
                        .spec
                        .resources
                        .requests
                        .storage,
                    format!("{initial_storage_size}Gi")
                );
            }
            Err(_) => panic!(),
        };

        let ret = environment_delete.delete_environment(&ea_delete, &infra_ctx_for_deletion);
        assert!(matches!(ret, TransactionResult::Ok));

        test_name.to_string()
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_container_with_udp_tcp_public_ports() {
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = function_name!());
        let _enter = span.enter();

        let logger = logger();

        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = aws_default_infra_config(&context_for_delete, logger.clone(), metrics_registry());

        let mut environment = helpers::environment::working_minimal_environment(&context);

        environment.applications = vec![];
        let service_id = Uuid::new_v4();
        let main_port = 443;
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
                socat UDP6-LISTEN:80,bind=[::],reuseaddr,fork exec:"/bin/cat" &
                socat TCP6-LISTEN:5432,bind=[::],reuseaddr,fork STDOUT &
                socat TCP6-LISTEN:443,bind=[::],reuseaddr,fork STDOUT
                "#
                .to_string(),
            ],
            entrypoint: None,
            cpu_request_in_mili: 250,
            cpu_limit_in_mili: 250,
            ram_request_in_mib: 250,
            ram_limit_in_mib: 250,
            min_instances: 1,
            max_instances: 1,
            public_domain: format!("{}.{}", service_id, infra_ctx.dns_provider().domain()),
            ports: vec![
                Port {
                    long_id: Uuid::new_v4(),
                    port: main_port,
                    is_default: true,
                    name: format!("p{}", main_port),
                    publicly_accessible: true,
                    protocol: Protocol::TCP,
                },
                Port {
                    long_id: Uuid::new_v4(),
                    port: 5432,
                    is_default: false,
                    name: "p5432".to_string(),
                    publicly_accessible: true,
                    protocol: Protocol::TCP,
                },
                Port {
                    long_id: Uuid::new_v4(),
                    port: 80,
                    is_default: false,
                    name: "p80".to_string(),
                    publicly_accessible: true,
                    protocol: Protocol::UDP,
                },
            ],
            storages: vec![],
            environment_vars_with_infos: btreemap! { "MY_VAR".to_string() => VariableInfo{value: base64::encode("my_value"), is_secret: false} },
            mounted_files: vec![],
            readiness_probe: Some(Probe {
                r#type: ProbeType::Tcp { host: None },
                port: main_port as u32,
                initial_delay_seconds: 30,
                timeout_seconds: 2,
                period_seconds: 10,
                success_threshold: 1,
                failure_threshold: 5,
            }),
            liveness_probe: Some(Probe {
                r#type: ProbeType::Tcp { host: None },
                port: main_port as u32,
                initial_delay_seconds: 30,
                timeout_seconds: 2,
                period_seconds: 10,
                success_threshold: 1,
                failure_threshold: 5,
            }),
            advanced_settings: Default::default(),
        }];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        // check we can connect on ports
        sleep(Duration::from_secs(30));
        let now = Instant::now();
        let timeout = Duration::from_secs(60 * 10);
        loop {
            if now.elapsed() > timeout {
                panic!("Cannot connect to endpoint before timeout of {:?}", timeout);
            }

            sleep(Duration::from_secs(10));

            // check we can connect on port
            let domain = format!("p443-{}.{}:{}", service_id, infra_ctx.dns_provider().domain(), main_port);
            if std::net::TcpStream::connect(domain).is_err() {
                continue;
            }

            // Check udp is echoing back our message
            let udp = UdpSocket::bind("[::]:0").expect("cannot bind udp socket");
            udp.connect(format!("p80-{}.{}:80", service_id, infra_ctx.dns_provider().domain()))
                .expect("cannot connect to udp socket");
            let _ = udp.set_nonblocking(true);

            loop {
                if now.elapsed() > timeout {
                    panic!("Cannot rcv udp hello msg before timeout of {:?}", timeout);
                }
                sleep(Duration::from_secs(10));

                udp.send(b"hello").expect("cannot send udp packet");
                let mut buf = [0; 10];

                if udp.recv(&mut buf).is_err() {
                    continue;
                }

                assert_eq!(&buf[0..5], b"hello");
                break;
            }

            // exit loop
            break;
        }

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        "".to_string()
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[test]
fn deploy_helm_chart() {
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = "deploy_helm_chart");
        let _enter = span.enter();

        let logger = logger();

        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = aws_default_infra_config(&context_for_delete, logger.clone(), metrics_registry());

        let mut environment = helpers::environment::working_minimal_environment(&context);

        environment.applications = vec![];
        let service_id = Uuid::new_v4();
        environment.helms = vec![HelmChart {
            long_id: service_id,
            name: "my little chart ****".to_string(),
            kube_name: "my-little-chart".to_string(),
            action: Action::Create,
            chart_source: HelmChartSource::Git {
                git_url: Url::parse("https://github.com/Qovery/helm_chart_engine_testing.git").unwrap(),
                git_credentials: None,
                commit_id: "c4c33c5f7f6e88e2a24c81883c8868c79bbfffb5".to_string(),
                root_path: PathBuf::from("/simple_app"),
            },
            chart_values: HelmValueSource::Raw {
                values: vec![HelmRawValues {
                    name: "toto.yaml".to_string(),
                    content: "nameOverride: tata".to_string(),
                }],
            },
            //chart_values: HelmValueSource::Git {
            //    git_url: Url::parse("https://github.com/erebe/test_http_server.git").unwrap(),
            //    git_credentials: None,
            //    commit_id: "753aa76982c710ee59db35e21669f6434ae4fa12".to_string(),
            //    values_path: vec![PathBuf::from(".github/workflows/docker-image.yml")],
            //},
            set_values: vec![
                ("toto".to_string(), "tata".to_string()),
                ("serviceId".to_string(), service_id.to_string()),
            ],
            set_string_values: vec![("my-string".to_string(), "1".to_string())],
            set_json_values: vec![("my-json".to_string(), "{\"json\": \"value\"}".to_string())],
            command_args: vec!["--install".to_string()],
            timeout_sec: 60,
            allow_cluster_wide_resources: false,
            environment_vars_with_infos: btreemap! { "TOTO".to_string() => VariableInfo {value: "Salut".to_string(), is_secret: false} },
            advanced_settings: Default::default(),
        }];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        "".to_string()
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_helm_chart_and_pause_it() {
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = "deploy_helm_chart");
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = aws_default_infra_config(&context_for_delete, logger.clone(), metrics_registry());

        let mut environment = helpers::environment::working_minimal_environment(&context);

        environment.applications = vec![];
        let service_id = Uuid::new_v4();
        println!("service id {}", service_id);
        environment.helms = vec![HelmChart {
            long_id: service_id,
            name: "my little chart ****".to_string(),
            kube_name: "my-little-chart".to_string(),
            action: Action::Create,
            chart_source: HelmChartSource::Git {
                git_url: Url::parse("https://github.com/Qovery/helm_chart_engine_testing.git").unwrap(),
                git_credentials: None,
                commit_id: "214310971046bc28db8c03b068248ed11b68315b".to_string(),
                root_path: PathBuf::from("/simple_app"),
            },
            chart_values: HelmValueSource::Raw {
                values: vec![HelmRawValues {
                    name: "toto.yaml".to_string(),
                    content: "nameOverride: tata".to_string(),
                }],
            },
            //chart_values: HelmValueSource::Git {
            //    git_url: Url::parse("https://github.com/erebe/test_http_server.git").unwrap(),
            //    git_credentials: None,
            //    commit_id: "753aa76982c710ee59db35e21669f6434ae4fa12".to_string(),
            //    values_path: vec![PathBuf::from(".github/workflows/docker-image.yml")],
            //},
            set_values: vec![("toto".to_string(), "tata".to_string())],
            set_string_values: vec![
                ("my-string".to_string(), "1".to_string()),
                ("serviceId".to_string(), service_id.to_string()),
            ],
            set_json_values: vec![("my-json".to_string(), "{\"json\": \"value\"}".to_string())],
            command_args: vec!["--install".to_string()],
            timeout_sec: 60,
            allow_cluster_wide_resources: false,
            environment_vars_with_infos: btreemap! { "TOTO".to_string() => VariableInfo {value: "Salut".to_string(), is_secret: false} },
            advanced_settings: Default::default(),
        }];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = get_pods(&infra_ctx, Kind::Aws, &environment.clone(), &service_id, secrets.clone());
        assert!(ret.is_ok());
        assert!(!ret.unwrap().items.is_empty());

        let ret = environment.pause_environment(&environment, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        // Check that we have actually 0 pods running for this app
        let ret = get_pods(&infra_ctx, Kind::Aws, &environment.clone(), &service_id, secrets.clone());
        assert!(ret.is_ok());
        assert!(ret.unwrap().items.is_empty());

        // Check we can resume the env
        let ctx_resume = context.clone_not_same_execution_id();
        let infra_ctx_resume = aws_default_infra_config(&ctx_resume, logger.clone(), metrics_registry());
        let ret = environment.deploy_environment(&environment, &infra_ctx_resume);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = get_pods(&infra_ctx, Kind::Aws, &environment.clone(), &service_id, secrets);
        assert!(ret.is_ok());
        assert!(!ret.unwrap().items.is_empty());

        // Cleanup
        let ret = environment.delete_environment(&environment, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        "".to_string()
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_helm_chart_and_restart_it() {
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = "deploy_helm_chart");
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = aws_default_infra_config(&context_for_delete, logger.clone(), metrics_registry());

        let mut environment = helpers::environment::working_minimal_environment(&context);

        environment.applications = vec![];
        let service_id = Uuid::new_v4();
        println!("service id {}", service_id);
        environment.helms = vec![HelmChart {
            long_id: service_id,
            name: "my little chart ****".to_string(),
            kube_name: "my-little-chart".to_string(),
            action: Action::Create,
            chart_source: HelmChartSource::Git {
                git_url: Url::parse("https://github.com/Qovery/helm_chart_engine_testing.git").unwrap(),
                git_credentials: None,
                commit_id: "214310971046bc28db8c03b068248ed11b68315b".to_string(),
                root_path: PathBuf::from("/simple_app"),
            },
            chart_values: HelmValueSource::Raw {
                values: vec![HelmRawValues {
                    name: "toto.yaml".to_string(),
                    content: "nameOverride: tata".to_string(),
                }],
            },
            //chart_values: HelmValueSource::Git {
            //    git_url: Url::parse("https://github.com/erebe/test_http_server.git").unwrap(),
            //    git_credentials: None,
            //    commit_id: "753aa76982c710ee59db35e21669f6434ae4fa12".to_string(),
            //    values_path: vec![PathBuf::from(".github/workflows/docker-image.yml")],
            //},
            set_values: vec![("toto".to_string(), "tata".to_string())],
            set_string_values: vec![
                ("my-string".to_string(), "1".to_string()),
                ("serviceId".to_string(), service_id.to_string()),
            ],
            set_json_values: vec![("my-json".to_string(), "{\"json\": \"value\"}".to_string())],
            command_args: vec!["--install".to_string()],
            timeout_sec: 60,
            allow_cluster_wide_resources: false,
            environment_vars_with_infos: btreemap! { "TOTO".to_string() => VariableInfo {value: "Salut".to_string(), is_secret: false} },
            advanced_settings: Default::default(),
        }];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = get_pods(&infra_ctx, Kind::Aws, &environment.clone(), &service_id, secrets.clone());
        assert!(ret.is_ok());
        assert!(!ret.unwrap().items.is_empty());

        let ret = environment.restart_environment(&environment, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        "".to_string()
    })
}

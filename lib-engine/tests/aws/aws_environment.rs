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
use qovery_engine::io_models::application::{Port, Protocol, Storage};

use base64::engine::general_purpose;
use base64::Engine;
use k8s_openapi::api::core::v1::ConfigMap;
use qovery_engine::io_models::annotations_group::{Annotation, AnnotationsGroup, AnnotationsGroupScope};
use qovery_engine::io_models::application::Protocol::HTTP;
use qovery_engine::io_models::container::{Container, Registry};
use qovery_engine::io_models::context::CloneForTest;
use qovery_engine::io_models::helm_chart::{HelmChart, HelmChartSource, HelmRawValues, HelmValueSource};
use qovery_engine::io_models::job::{ContainerRegistries, Job, JobSchedule, JobSource, LifecycleType};
use qovery_engine::io_models::labels_group::{Label, LabelsGroup};
use qovery_engine::io_models::probe::{Probe, ProbeType};
use qovery_engine::io_models::router::{CustomDomain, Route, Router};
use qovery_engine::io_models::variable_utils::VariableInfo;
use qovery_engine::io_models::{Action, MountedFile, QoveryIdentifier};
use qovery_engine::metrics_registry::{StepLabel, StepName, StepStatus};
use qovery_engine::models::aws::AwsStorageType;
use qovery_engine::runtime::block_on;
use qovery_engine::transaction::TransactionResult;
use qovery_engine::utilities::to_short_id;
use reqwest::StatusCode;
use retry::delay::Fibonacci;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
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
    // basically building and pushing each application
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
            .image_exists(&env.applications[0].get_build().image);
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
            .image_exists(&env.applications[0].get_build().image);
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
        assert_eq!(records.len(), 6);

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

        // The start of the Total record has been moved to the EnvironmentTask.run() method. As a result, it is no longer invoked during test execution.

        // let record_total = records.iter().find(|step| step.step_name == StepName::Total).unwrap();
        // assert_eq!(record_total.step_name, StepName::Total);
        // assert_eq!(record_total.label, StepLabel::Service);
        // assert_eq!(record_total.id, environment.applications.first().unwrap().long_id);
        // assert_eq!(record_total.status, Some(StepStatus::Success));
        // assert!(record_total.duration.is_some());

        let queueing_total = records
            .iter()
            .find(|step| step.step_name == StepName::DeploymentQueueing)
            .unwrap();
        assert_eq!(queueing_total.step_name, StepName::DeploymentQueueing);
        assert_eq!(queueing_total.label, StepLabel::Service);
        assert_eq!(queueing_total.id, environment.applications.first().unwrap().long_id);
        assert_eq!(queueing_total.status, Some(StepStatus::Success));
        assert!(queueing_total.duration.is_some());

        // TODO(benjaminch): to be fixed / updated
        // let records = metrics_registry_for_deployment.get_records(environment.long_id);
        // assert_eq!(records.len(), 1);

        // The start of the Total record has been moved to the EnvironmentTask.run() method. As a result, it is no longer invoked during test execution.

        // let record_total = records.iter().find(|step| step.step_name == StepName::Total).unwrap();
        // assert_eq!(record_total.step_name, StepName::Total);
        // assert_eq!(record_total.label, StepLabel::Environment);
        // assert_eq!(record_total.id, environment.long_id);
        // assert_eq!(record_total.status, Some(StepStatus::Success));
        // assert!(record_total.duration.is_some());

        // TODO(benjaminch): to be fixed / updated
        // let record_provision = records
        //     .iter()
        //     .find(|step| step.step_name == StepName::ProvisionBuilder)
        //     .unwrap();
        // assert_eq!(record_provision.step_name, StepName::ProvisionBuilder);
        // assert_eq!(record_provision.label, StepLabel::Environment);
        // assert_eq!(record_provision.id, environment.long_id);
        // assert_eq!(record_provision.status, Some(StepStatus::Success));
        // assert!(record_provision.duration.is_some());

        let ret = environment_for_delete.delete_environment(&ea_delete, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        test_name.to_string()
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_a_working_environment_with_shared_registry() {
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

        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry_for_deployment.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete = aws_default_infra_config(&context_for_delete, logger.clone(), metrics_registry());

        let environment =
            helpers::environment::working_minimal_environment_with_custom_git_url(&context, repo_url.as_str());
        let environment2 =
            helpers::environment::working_minimal_environment_with_custom_git_url(&context, repo_url.as_str());

        let env_to_deploy = environment.clone();
        let env_to_deploy2 = environment2.clone();

        let ret = environment.deploy_environment(&env_to_deploy, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));
        let ret = environment2.deploy_environment(&env_to_deploy2, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));
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
        assert!(matches!(ret, TransactionResult::Ok));
        let img_exist = infra_ctx
            .container_registry()
            .image_exists(&env.applications[0].get_build().image);
        assert!(img_exist);
        let ret = environment_to_delete2.delete_environment(&environment_to_delete2, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));
        let img_exist = infra_ctx
            .container_registry()
            .image_exists(&env.applications[0].get_build().image);
        assert!(!img_exist);
        // stick a guard on the repository to delete after test

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
#[ignore = "Buildpacks to be deactivated soon PRDT-1339"]
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
                    protocol: HTTP,
                    service_name: None,
                    namespace: None,
                    additional_service: None,
                }];
                app.commit_id = "8fa91f8d44de4c88b065fd0897e6c71b44093bc1".to_string();
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
#[ignore = "Buildpacks to be deactivated soon PRDT-1339"]
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
                    protocol: HTTP,
                    service_name: None,
                    namespace: None,
                    additional_service: None,
                }];
                app.commit_id = "8fa91f8d44de4c88b065fd0897e6c71b44093bc1".to_string();
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
                    storage_class: AwsStorageType::GP2.to_k8s_storage_class(),
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
            file_content_b64: general_purpose::STANDARD.encode("I exist !"),
        };

        let environment =
            helpers::environment::working_environment_with_application_and_stateful_crashing_if_file_doesnt_exist(
                &context,
                &mounted_file,
                &AwsStorageType::GP2.to_k8s_storage_class(),
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
            infra_ctx
                .mk_kube_client()
                .expect("kube client is not set")
                .client()
                .clone(),
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
                    storage_class: AwsStorageType::GP2.to_k8s_storage_class(),
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

        let span = span!(Level::INFO, "test", function_name!());
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
        let kubeconfig = infra_ctx.kubernetes().kubeconfig_local_file_path();
        let router = environment
            .routers
            .first()
            .unwrap()
            .to_router_domain(
                infra_ctx.context(),
                RouterAdvancedSettings::default(),
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
        let host_suffix = Uuid::new_v4();
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
            ports: vec![
                Port {
                    long_id: Uuid::new_v4(),
                    port: 8080,
                    is_default: true,
                    name: format!("p8080-{}", host_suffix),
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
                    name: format!("grpc-{}", host_suffix),
                    publicly_accessible: false,
                    protocol: HTTP,
                    service_name: None,
                    namespace: None,
                    additional_service: None,
                },
            ],
            storages: vec![],
            environment_vars_with_infos: btreemap! { "MY_VAR".to_string() => VariableInfo { value: general_purpose::STANDARD.encode("my_value"), is_secret: false}},
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
            annotations_group_ids: btreeset! {},
            labels_group_ids: btreeset! {},
        }];

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        let kube_conn = infra_ctx.mk_kube_client().expect("kube client is not set");
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
                .node_selector_terms
                .into_iter()
                .find(|node_selector| {
                    node_selector
                        .match_expressions
                        .clone()
                        .unwrap()
                        .iter()
                        .any(|selector| selector.key == node_selector_key)
                })
                .unwrap()
                .clone()
                .match_expressions
                .unwrap();
            let nf = node_affinity
                .iter()
                .find(|node_affinity| node_affinity.key == node_selector_key);
            assert_ne!(nf, None);
            assert_eq!(
                <Option<Vec<String>> as Clone>::clone(&nf.unwrap().values).unwrap()[0],
                node_selector_value
            );
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
                    name: "p8080".to_string(),
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
            storages: vec![],
            environment_vars_with_infos: btreemap! { "MY_VAR".to_string() => VariableInfo { value: general_purpose::STANDARD.encode("my_value"), is_secret: false} },
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
            annotations_group_ids: btreeset! {},
            labels_group_ids: btreeset! {},
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
                    storage_class: AwsStorageType::GP2.to_k8s_storage_class(),
                    size_in_gib: 10,
                    mount_point: "/mnt/photos1".to_string(),
                    snapshot_retention_in_days: 0,
                },
                Storage {
                    id: to_short_id(&storage_id_2),
                    long_id: storage_id_2,
                    name: "photos2".to_string(),
                    storage_class: AwsStorageType::GP2.to_k8s_storage_class(),
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
            infra_ctx
                .mk_kube_client()
                .expect("kube client is not set")
                .client()
                .clone(),
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
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        "".to_string()
    })
}

#[cfg(feature = "test-aws-minimal")]
#[named]
#[test]
fn deploy_job_on_aws_eks() {
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", function_name!());
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
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        "".to_string()
    })
}

#[cfg(feature = "test-aws-minimal")]
#[named]
#[test]
fn deploy_job_on_aws_eks_with_dockerfile_content() {
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", function_name!());
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
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        "".to_string()
    })
}

#[cfg(feature = "test-aws-minimal")]
#[named]
#[test]
fn deploy_cronjob_on_aws_eks() {
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", function_name!());
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
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        "".to_string()
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_cronjob_force_trigger_on_aws_eks() {
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", function_name!());
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
                .mk_kube_client()
                .expect("should always contain kube_client")
                .client()
                .clone(),
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
#[named]
#[test]
fn build_and_deploy_job_on_aws_eks() {
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", function_name!());
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
                socat TCP-LISTEN:8080,bind=0.0.0.0,reuseaddr,fork STDOUT
                "#
                .to_string(),
            ],
            entrypoint: None,
            cpu_request_in_milli: 250,
            cpu_limit_in_milli: 250,
            ram_request_in_mib: 250,
            ram_limit_in_mib: 250,
            min_instances: 2,
            max_instances: 2,
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
            environment_vars_with_infos: btreemap! { "MY_VAR".to_string() => VariableInfo{value: general_purpose::STANDARD.encode("my_value"), is_secret: false} },
            advanced_settings: Default::default(),
            annotations_group_ids: btreeset! {},
            labels_group_ids: btreeset! {},
        }];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        sleep(Duration::from_secs(20));

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
                socat TCP-LISTEN:8080,bind=0.0.0.0,reuseaddr,fork STDOUT
                "#
                .to_string(),
            ],
            storages: vec![Storage {
                id: "z111111".to_string(),
                long_id: Uuid::new_v4(),
                name: "storage-1".to_string(),
                mount_point: "/storage".to_string(),
                size_in_gib: 10,
                storage_class: AwsStorageType::GP2.to_k8s_storage_class(),
                snapshot_retention_in_days: 1,
            }],
            mounted_files: vec![],
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
            environment_vars_with_infos: btreemap! { "MY_VAR".to_string() => VariableInfo{value: general_purpose::STANDARD.encode("my_value"), is_secret: false} },
            advanced_settings: Default::default(),
            annotations_group_ids: btreeset! {},
            labels_group_ids: btreeset! {},
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
#[named]
#[test]
fn build_and_deploy_job_on_aws_eks_with_mounted_files_as_volume() {
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", function_name!());
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
            file_content_b64: general_purpose::STANDARD
                .encode(r#"{"foo": {"value": "bar", "sensitive": true}, "foo_2": {"value": "bar_2"}}"#),
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
                commit_id: "d22414a253db2bcf3acf91f85565d2dabe9211cc".to_string(),
                dockerfile_path: Some("Dockerfile".to_string()),
                root_path: String::from("/"),
                git_credentials: None,
                branch: "main".to_string(),
                dockerfile_content: None,
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
            infra_ctx
                .mk_kube_client()
                .expect("kube client is not set")
                .client()
                .clone(),
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
                        storage_class: AwsStorageType::GP2.to_k8s_storage_class(),
                        size_in_gib: initial_storage_size,
                        mount_point: "/mnt/photos_1".to_string(),
                        snapshot_retention_in_days: 0,
                    },
                    Storage {
                        id: to_short_id(&id_2),
                        long_id: id_2,
                        name: "photos_2".to_string(),
                        storage_class: AwsStorageType::GP2.to_k8s_storage_class(),
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
    use qovery_engine::cloud_provider::utilities::{check_tcp_port_is_open, check_udp_port_is_open, TcpCheckSource};
    use tracing::info;

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
        let tcp_port = 443;
        let udp_port = 80;
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
                format!(
                    r#"
                apt-get update;
                apt-get install -y socat procps iproute2;
                echo listening on port $PORT;
                env
                socat UDP6-LISTEN:{},bind=[::],reuseaddr,fork exec:"/bin/cat" &
                socat TCP6-LISTEN:5432,bind=[::],reuseaddr,fork STDOUT &
                socat TCP6-LISTEN:{},bind=[::],reuseaddr,fork STDOUT
                "#,
                    udp_port, tcp_port
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
                    port: tcp_port,
                    is_default: true,
                    name: format!("p{}", tcp_port),
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
                    port: udp_port,
                    is_default: false,
                    name: format!("p{}", udp_port),
                    publicly_accessible: true,
                    protocol: Protocol::UDP,
                    service_name: None,
                    namespace: None,
                    additional_service: None,
                },
            ],
            storages: vec![],
            environment_vars_with_infos: btreemap! { "MY_VAR".to_string() => VariableInfo{value: general_purpose::STANDARD.encode("my_value"), is_secret: false} },
            mounted_files: vec![],
            readiness_probe: Some(Probe {
                r#type: ProbeType::Tcp { host: None },
                port: tcp_port as u32,
                initial_delay_seconds: 30,
                timeout_seconds: 2,
                period_seconds: 10,
                success_threshold: 1,
                failure_threshold: 5,
            }),
            liveness_probe: Some(Probe {
                r#type: ProbeType::Tcp { host: None },
                port: tcp_port as u32,
                initial_delay_seconds: 30,
                timeout_seconds: 2,
                period_seconds: 10,
                success_threshold: 1,
                failure_threshold: 5,
            }),
            advanced_settings: Default::default(),
            annotations_group_ids: btreeset! {},
            labels_group_ids: btreeset! {},
        }];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        // check we can connect on ports
        sleep(Duration::from_secs(30));
        let now = Instant::now();
        let timeout = Duration::from_secs(60 * 10);
        let tcp_domain = format!("p{}-{}.{}", tcp_port, service_id, infra_ctx.dns_provider().domain());
        let udp_domain = format!("p{}-{}.{}", udp_port, service_id, infra_ctx.dns_provider().domain());
        loop {
            if now.elapsed() > timeout {
                panic!("Cannot connect to endpoint before timeout of {:?}", timeout);
            }

            sleep(Duration::from_secs(10));

            // check we can connect on port
            if check_tcp_port_is_open(&TcpCheckSource::DnsName(&tcp_domain), tcp_port).is_err() {
                info!("Cannot connect to {} port {}/tcp yet...", tcp_domain, tcp_port);
                continue;
            }

            if check_udp_port_is_open(&udp_domain, udp_port).is_err() {
                info!("Cannot connect to {} port {}/udp yet...", udp_domain, udp_port);
                continue;
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
#[named]
#[test]
fn deploy_helm_chart() {
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", function_name!());
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
                commit_id: "18679eb4acf787470d4e3bdd4aa369c7dcea90a0".to_string(),
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
            ports: vec![],
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
// 1. Deploy helm chart
// 2. Check admission controller config map is created with good info
// 3. Redeploy helm chart with different version
// 2. Check admission controller config map is updated with new version
fn deploy_helm_chart_twice_to_check_admission_controller_config_map_is_well_created_and_updated() {
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", function_name!());
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
                commit_id: "18679eb4acf787470d4e3bdd4aa369c7dcea90a0".to_string(),
                root_path: PathBuf::from("/simple_app"),
            },
            chart_values: HelmValueSource::Raw {
                values: vec![HelmRawValues {
                    name: "toto.yaml".to_string(),
                    content: "nameOverride: tata".to_string(),
                }],
            },
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
            ports: vec![],
        }];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // Deploy the helm chart and check config map is well created
        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        let kube_client = infra_ctx
            .mk_kube_client()
            .expect("kube client is not set")
            .client()
            .clone();
        let api_config_map: Api<ConfigMap> = Api::namespaced(kube_client, &environment.kube_name);
        let short_id = to_short_id(&service_id);
        let config_map_name = format!("{short_id}-admission-controller-config-map");

        let config_map: ConfigMap = block_on(api_config_map.get(&config_map_name)).unwrap();
        let config_map_data = config_map.data.unwrap();
        assert_eq!(config_map_data.len(), 5);
        let config_map_project_id = config_map_data.get("project-id").expect("Cannot find project-id");
        let config_map_environment_id = config_map_data
            .get("environment-id")
            .expect("Cannot find environment-id");
        let config_map_service_id = config_map_data.get("service-id").expect("Cannot find service-id");
        let config_map_service_version = config_map_data
            .get("service-version")
            .expect("Cannot find service-version");
        assert_eq!(
            &config_map_project_id.to_string(),
            environment.project_long_id.to_string().as_str()
        );
        assert_eq!(&config_map_environment_id.to_string(), environment.long_id.to_string().as_str());
        assert_eq!(&config_map_service_id.to_string(), service_id.to_string().as_str());
        assert_eq!(
            &config_map_service_version.to_string(),
            "18679eb4acf787470d4e3bdd4aa369c7dcea90a0".to_string().as_str()
        );

        // Redeploy helm chart
        environment.helms = vec![HelmChart {
            long_id: service_id,
            name: "my little chart ****".to_string(),
            kube_name: "my-little-chart".to_string(),
            action: Action::Create,
            chart_source: HelmChartSource::Git {
                git_url: Url::parse("https://github.com/Qovery/helm_chart_engine_testing.git").unwrap(),
                git_credentials: None,
                commit_id: "b93c8d1b9c0bea63f7ce6a669c758cd6b9c9ece2".to_string(),
                root_path: PathBuf::from("/simple_app"),
            },
            chart_values: HelmValueSource::Raw {
                values: vec![HelmRawValues {
                    name: "toto.yaml".to_string(),
                    content: "nameOverride: tata".to_string(),
                }],
            },
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
            ports: vec![],
        }];

        // Delete helm chart dir otherwise it would fail
        let chart_directory = qovery_engine::fs::workspace_directory(
            context.workspace_root_dir(),
            context.execution_id(),
            format!("helm_charts/{service_id}"),
        )
        .unwrap();
        let chart_dir = chart_directory.to_str().unwrap();
        std::fs::remove_dir_all(Path::new(chart_dir)).unwrap();

        let ret = environment.deploy_environment(&environment, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        let config_map: ConfigMap = block_on(api_config_map.get(&config_map_name)).unwrap();
        let config_map_data = config_map.data.unwrap();
        let config_map_service_version = config_map_data
            .get("service-version")
            .expect("Cannot find service-version");
        assert_eq!(
            &config_map_service_version.to_string(),
            "b93c8d1b9c0bea63f7ce6a669c758cd6b9c9ece2".to_string().as_str()
        );

        let ret = environment_for_delete.delete_environment(&environment_for_delete, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        "".to_string()
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_helm_chart_and_pause_it() {
    test_deploy_helm_chart_and_pause_it(true);
    test_deploy_helm_chart_and_pause_it(false);

    fn test_deploy_helm_chart_and_pause_it(allow_cluster_wide_resources: bool) {
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
            let infra_ctx_for_delete =
                aws_default_infra_config(&context_for_delete, logger.clone(), metrics_registry());

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
                allow_cluster_wide_resources,
                environment_vars_with_infos: btreemap! { "TOTO".to_string() => VariableInfo {value: "Salut".to_string(), is_secret: false} },
                advanced_settings: Default::default(),
                ports: vec![],
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
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_helm_chart_and_restart_it() {
    test_deploy_helm_chart_and_restart_it(true);
    test_deploy_helm_chart_and_restart_it(false);

    fn test_deploy_helm_chart_and_restart_it(allow_cluster_wide_resources: bool) {
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
            let infra_ctx_for_delete =
                aws_default_infra_config(&context_for_delete, logger.clone(), metrics_registry());

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
                allow_cluster_wide_resources,
                environment_vars_with_infos: btreemap! { "TOTO".to_string() => VariableInfo {value: "Salut".to_string(), is_secret: false} },
                advanced_settings: Default::default(),
                ports: vec![],
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
}

#[cfg(feature = "test-aws-self-hosted")]
#[test]
fn deploy_helm_chart_with_router() {
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = "deploy_helm_chart");
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .AWS_TEST_ORGANIZATION_LONG_ID
                .expect("AWS_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .AWS_TEST_CLUSTER_LONG_ID
                .expect("AWS_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = aws_default_infra_config(&context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete =
            aws_default_infra_config(&context_for_delete, logger.clone(), metrics_registry.clone());

        let mut environment = helpers::environment::working_minimal_environment(&context);

        // generate an extra namespace to deploy a service and ingress
        let extra_namespace = format!("extra-env-{}", Uuid::new_v4());
        let host_suffix = Uuid::new_v4();

        environment.applications = vec![];
        let service_id = Uuid::new_v4();
        environment.helms = vec![HelmChart {
            long_id: service_id,
            name: "my special chart ****".to_string(),
            kube_name: "my-special-chart".to_string(),
            action: Action::Create,
            chart_source: HelmChartSource::Git {
                git_url: Url::parse("https://github.com/Qovery/helm_chart_engine_testing.git").unwrap(),
                git_credentials: None,
                commit_id: "8acb6e06d98c0c1b8f2285d5c5bc7f1a837a782a".to_string(),
                root_path: PathBuf::from("/several_services"),
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
                ("service2.namespace".to_string(), extra_namespace.clone()),
                ("serviceId".to_string(), service_id.to_string()),
            ],
            set_string_values: vec![],
            set_json_values: vec![],
            command_args: vec![],
            timeout_sec: 60,
            allow_cluster_wide_resources: true,
            environment_vars_with_infos: btreemap! { "TOTO".to_string() => VariableInfo {value: "Salut".to_string(), is_secret: false} },
            advanced_settings: Default::default(),
            ports: vec![
                Port {
                    long_id: Uuid::new_v4(),
                    port: 8080,
                    is_default: false,
                    name: format!("service1-p8080-{}", host_suffix),
                    publicly_accessible: true,
                    protocol: Protocol::HTTP,
                    namespace: None,
                    service_name: Some("inner-namespace-service1".to_string()),
                    additional_service: None,
                },
                Port {
                    long_id: Uuid::new_v4(),
                    port: 8080,
                    is_default: false,
                    name: format!("service2-p8080-{}", host_suffix),
                    publicly_accessible: true,
                    protocol: Protocol::HTTP,
                    namespace: Some(extra_namespace.clone()),
                    service_name: Some("outside-namespace-service2".to_string()),
                    additional_service: None,
                },
            ],
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
                service_long_id: environment.helms[0].long_id,
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

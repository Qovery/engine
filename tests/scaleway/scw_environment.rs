extern crate test_utilities;

use self::test_utilities::cloudflare;
use self::test_utilities::scaleway::SCW_TEST_CLUSTER_ID;
use self::test_utilities::utilities::{engine_run_test, generate_id, get_pods, FuncTestsSecrets};
use qovery_engine::cloud_provider::scaleway::application::Region;
use qovery_engine::cloud_provider::Kind;
use qovery_engine::models::{Action, Clone2, Context, EnvironmentAction, Storage, StorageType};
use qovery_engine::transaction::{DeploymentOption, TransactionResult};
use std::str::FromStr;
use test_utilities::utilities::context;
use test_utilities::utilities::init;
use tracing::{span, Level};

// Note: All those tests relies on a test cluster running on Scaleway infrastructure.
// This cluster should be live in order to have those tests passing properly.

pub fn deploy_environment(context: &Context, environment_action: EnvironmentAction) -> TransactionResult {
    let engine = test_utilities::scaleway::docker_scw_cr_engine(context);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let cp = test_utilities::scaleway::cloud_provider_scaleway(context);
    let nodes = test_utilities::scaleway::scw_kubernetes_nodes();
    let dns_provider = test_utilities::cloudflare::dns_provider_cloudflare(context);
    let kapsule = test_utilities::scaleway::scw_kubernetes_kapsule(context, &cp, &dns_provider, nodes);

    let _ = tx.deploy_environment_with_options(
        &kapsule,
        &environment_action,
        DeploymentOption {
            force_build: true,
            force_push: true,
        },
    );

    tx.commit()
}

pub fn delete_environment(context: &Context, environment_action: EnvironmentAction) -> TransactionResult {
    let engine = test_utilities::scaleway::docker_scw_cr_engine(context);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let cp = test_utilities::scaleway::cloud_provider_scaleway(context);
    let nodes = test_utilities::scaleway::scw_kubernetes_nodes();
    let dns_provider = test_utilities::cloudflare::dns_provider_cloudflare(context);
    let kapsule = test_utilities::scaleway::scw_kubernetes_kapsule(context, &cp, &dns_provider, nodes);

    let _ = tx.delete_environment(&kapsule, &environment_action);

    tx.commit()
}

pub fn pause_environment(context: &Context, environment_action: EnvironmentAction) -> TransactionResult {
    let engine = test_utilities::scaleway::docker_scw_cr_engine(context);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let cp = test_utilities::scaleway::cloud_provider_scaleway(context);
    let nodes = test_utilities::scaleway::scw_kubernetes_nodes();
    let dns_provider = test_utilities::cloudflare::dns_provider_cloudflare(context);
    let kapsule = test_utilities::scaleway::scw_kubernetes_kapsule(context, &cp, &dns_provider, nodes);

    let _ = tx.pause_environment(&kapsule, &environment_action);

    tx.commit()
}

#[cfg(feature = "test-scw-self-hosted")]
#[test]
fn scaleway_kapsule_deploy_a_working_environment_with_no_router() {
    engine_run_test(|| {
        let test_name = "scaleway_kapsule_deploy_a_working_environment_with_no_router";
        let span = span!(Level::INFO, "test", name = test_name,);
        let _enter = span.enter();

        let context = context();
        let context_for_delete = context.clone_not_same_execution_id();
        let secrets = FuncTestsSecrets::new();
        let mut environment = test_utilities::scaleway::working_minimal_environment(&context, secrets.clone());

        let mut environment_for_delete = environment.clone();
        environment.routers = vec![];
        environment_for_delete.routers = vec![];
        environment_for_delete.action = Action::Delete;

        let env_action = EnvironmentAction::Environment(environment);
        let env_action_for_delete = EnvironmentAction::Environment(environment_for_delete);
        let region = Region::from_str(secrets.SCALEWAY_DEFAULT_REGION.unwrap().as_str()).unwrap();

        match deploy_environment(&context, env_action) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        match delete_environment(&context_for_delete, env_action_for_delete) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[test]
fn scaleway_kapsule_deploy_a_not_working_environment_with_no_router() {
    engine_run_test(|| {
        let test_name = "scaleway_kapsule_deploy_a_not_working_environment_with_no_router";
        let span = span!(Level::INFO, "test", name = test_name,);
        let _enter = span.enter();

        let context = context();
        let context_for_delete = context.clone_not_same_execution_id();
        let secrets = FuncTestsSecrets::new();

        let mut environment = test_utilities::scaleway::non_working_environment(&context, secrets.clone());
        environment.routers = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let env_action = EnvironmentAction::Environment(environment);
        let env_action_for_delete = EnvironmentAction::Environment(environment_for_delete);

        match deploy_environment(&context, env_action) {
            TransactionResult::Ok => assert!(false),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(true),
        };

        match delete_environment(&context_for_delete, env_action_for_delete) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(true),
        };

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[test]
fn scaleway_kapsule_deploy_a_working_environment_and_pause() {
    engine_run_test(|| {
        let test_name = "scaleway_kapsule_deploy_a_working_environment_and_pause";
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let context = context();
        let context_for_delete = context.clone_not_same_execution_id();
        let secrets = FuncTestsSecrets::new();
        let environment = test_utilities::scaleway::working_minimal_environment(&context, secrets.clone());

        let env_action = EnvironmentAction::Environment(environment.clone());

        match deploy_environment(&context, env_action.clone()) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        match pause_environment(&context_for_delete, env_action.clone()) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        // Check that we have actually 0 pods running for this app
        let app_name = format!("{}-0", environment.applications[0].name);
        let ret = get_pods(
            Kind::Scw,
            context.clone(),
            environment.clone(),
            app_name.clone().as_str(),
            SCW_TEST_CLUSTER_ID,
            secrets.clone(),
        );
        assert_eq!(ret.is_ok(), true);
        assert_eq!(ret.unwrap().items.is_empty(), true);

        // Check we can resume the env
        let ctx_resume = context.clone_not_same_execution_id();
        match deploy_environment(&ctx_resume, env_action.clone()) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        // Cleanup
        match delete_environment(&context_for_delete, env_action.clone()) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[test]
fn scaleway_kapsule_build_with_buildpacks_and_deploy_a_working_environment() {
    engine_run_test(|| {
        let test_name = "scaleway_kapsule_build_with_buildpacks_and_deploy_a_working_environment";
        let span = span!(Level::INFO, "test", name = test_name,);
        let _enter = span.enter();

        let context = context();
        let context_for_delete = context.clone_not_same_execution_id();
        let secrets = FuncTestsSecrets::new();
        let mut environment = test_utilities::scaleway::working_minimal_environment(&context, secrets.clone());
        environment.applications = environment
            .applications
            .into_iter()
            .map(|mut app| {
                app.private_port = Some(3000);
                app.commit_id = "f59237d603829636138e2f22a0549e33b5dd6e1f".to_string();
                app.branch = "simple-node-app".to_string();
                app.dockerfile_path = None;
                app
            })
            .collect::<Vec<qovery_engine::models::Application>>();

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let env_action = EnvironmentAction::Environment(environment);
        let env_action_for_delete = EnvironmentAction::Environment(environment_for_delete);

        match deploy_environment(&context, env_action) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        match delete_environment(&context_for_delete, env_action_for_delete) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[test]
fn scaleway_kapsule_deploy_a_working_environment_with_domain() {
    engine_run_test(|| {
        let test_name = "scaleway_kapsule_deploy_a_working_environment_with_domain";
        let span = span!(Level::INFO, "test",);
        let _enter = span.enter();

        let context = context();
        let context_for_delete = context.clone_not_same_execution_id();
        let secrets = FuncTestsSecrets::new();
        let environment = test_utilities::aws::working_minimal_environment(&context, secrets.clone());

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;

        let env_action = EnvironmentAction::Environment(environment);
        let env_action_for_delete = EnvironmentAction::Environment(environment_delete);

        match deploy_environment(&context, env_action) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        match delete_environment(&context_for_delete, env_action_for_delete) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[test]
fn scaleway_kapsule_deploy_a_working_environment_with_storage() {
    engine_run_test(|| {
        let test_name = "scaleway_kapsule_deploy_a_working_environment_with_storage";
        let span = span!(Level::INFO, "test", name = test_name,);
        let _enter = span.enter();

        let context = context();
        let context_for_deletion = context.clone_not_same_execution_id();
        let secrets = FuncTestsSecrets::new();

        let mut environment = test_utilities::scaleway::working_minimal_environment(&context, secrets);

        // Todo: make an image that check there is a mounted disk
        environment.applications = environment
            .applications
            .into_iter()
            .map(|mut app| {
                app.storage = vec![Storage {
                    id: generate_id(),
                    name: "photos".to_string(),
                    storage_type: StorageType::Ssd,
                    size_in_gib: 10,
                    mount_point: "/mnt/photos".to_string(),
                    snapshot_retention_in_days: 0,
                }];
                app
            })
            .collect::<Vec<qovery_engine::models::Application>>();

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;

        let env_action = EnvironmentAction::Environment(environment);
        let env_action_delete = EnvironmentAction::Environment(environment_delete);

        match deploy_environment(&context, env_action) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        // todo: check the disk is here and with correct size

        match delete_environment(&context_for_deletion, env_action_delete) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        test_name.to_string()
    })
}

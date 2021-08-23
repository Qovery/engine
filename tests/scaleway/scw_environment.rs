extern crate test_utilities;

use self::test_utilities::scaleway::SCW_TEST_CLUSTER_ID;
use self::test_utilities::utilities::{
    engine_run_test, generate_id, get_pods, init, is_pod_restarted_env, FuncTestsSecrets,
};
use ::function_name::named;
use qovery_engine::build_platform::Image;
use qovery_engine::cloud_provider::scaleway::application::Region;
use qovery_engine::cloud_provider::Kind;
use qovery_engine::container_registry::scaleway_container_registry::ScalewayCR;
use qovery_engine::error::EngineError;
use qovery_engine::models::{Action, Clone2, Context, EnvironmentAction, Storage, StorageType};
use qovery_engine::transaction::{DeploymentOption, TransactionResult};
use scaleway_api_rs::models::ScalewayRegistryV1Namespace;
use std::str::FromStr;
use test_utilities::utilities::context;
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

pub fn delete_container_registry(
    context: Context,
    container_registry_name: &str,
    secrets: FuncTestsSecrets,
) -> Result<ScalewayRegistryV1Namespace, EngineError> {
    let secret_token = secrets.SCALEWAY_SECRET_KEY.unwrap();
    let project_id = secrets.SCALEWAY_DEFAULT_PROJECT_ID.unwrap();
    let region = Region::from_str(secrets.SCALEWAY_DEFAULT_REGION.unwrap().as_str()).unwrap();

    let container_registry_client = ScalewayCR::new(
        context.clone(),
        "test",
        "test",
        secret_token.as_str(),
        project_id.as_str(),
        region,
    );

    container_registry_client.delete_registry_namespace(&Image {
        name: container_registry_name.to_string(),
        ..Default::default()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn scaleway_kapsule_deploy_a_working_environment_with_no_router() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let context = context();
        let context_for_delete = context.clone_not_same_execution_id();
        let secrets = FuncTestsSecrets::new();
        let mut environment = test_utilities::scaleway::working_minimal_environment(&context, secrets.clone());

        let mut environment_for_delete = environment.clone();
        environment.routers = vec![];
        environment_for_delete.routers = vec![];
        environment_for_delete.action = Action::Delete;

        let env_action = EnvironmentAction::Environment(environment.clone());
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

        // delete container registries created during the test
        for app in environment.applications.iter() {
            assert_eq!(
                true,
                delete_container_registry(context.clone(), app.name.as_str(), secrets.clone()).is_ok()
            );
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn scaleway_kapsule_deploy_a_not_working_environment_with_no_router() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name,);
        let _enter = span.enter();

        let context = context();
        let context_for_delete = context.clone_not_same_execution_id();
        let secrets = FuncTestsSecrets::new();

        let mut environment = test_utilities::scaleway::non_working_environment(&context, secrets.clone());
        environment.routers = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let env_action = EnvironmentAction::Environment(environment.clone());
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

        // delete container registries created during the test
        for app in environment.applications.iter() {
            assert_eq!(
                true,
                delete_container_registry(context.clone(), app.name.as_str(), secrets.clone()).is_ok()
            );
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
        init();

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

        // delete container registries created during the test
        for app in environment.applications.iter() {
            assert_eq!(
                true,
                delete_container_registry(context.clone(), app.name.as_str(), secrets.clone()).is_ok()
            );
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn scaleway_kapsule_build_with_buildpacks_and_deploy_a_working_environment() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

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

        let env_action = EnvironmentAction::Environment(environment.clone());
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

        // delete container registries created during the test
        for app in environment.applications.iter() {
            assert_eq!(
                true,
                delete_container_registry(context.clone(), app.name.as_str(), secrets.clone()).is_ok()
            );
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
        init();

        let span = span!(Level::INFO, "test",);
        let _enter = span.enter();

        let context = context();
        let context_for_delete = context.clone_not_same_execution_id();
        let secrets = FuncTestsSecrets::new();
        let environment = test_utilities::scaleway::working_minimal_environment(&context, secrets.clone());

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;

        let env_action = EnvironmentAction::Environment(environment.clone());
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

        // delete container registries created during the test
        for app in environment.applications.iter() {
            assert_eq!(
                true,
                delete_container_registry(context.clone(), app.name.as_str(), secrets.clone()).is_ok()
            );
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn scaleway_kapsule_deploy_a_working_environment_with_storage() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name,);
        let _enter = span.enter();

        let context = context();
        let context_for_deletion = context.clone_not_same_execution_id();
        let secrets = FuncTestsSecrets::new();

        let mut environment = test_utilities::scaleway::working_minimal_environment(&context, secrets.clone());

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

        let env_action = EnvironmentAction::Environment(environment.clone());
        let env_action_delete = EnvironmentAction::Environment(environment_delete);

        match deploy_environment(&context, env_action) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        // TODO(benjaminch): check the disk is here and with correct size, can use Scaleway API

        match delete_environment(&context_for_deletion, env_action_delete) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        // delete container registries created during the test
        for app in environment.applications.iter() {
            assert_eq!(
                true,
                delete_container_registry(context.clone(), app.name.as_str(), secrets.clone()).is_ok()
            );
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn scaleway_kapsule_redeploy_same_app() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let context = context();
        let context_bis = context.clone_not_same_execution_id();
        let context_for_deletion = context.clone_not_same_execution_id();
        let secrets = FuncTestsSecrets::new();

        let mut environment = test_utilities::scaleway::working_minimal_environment(&context, secrets.clone());

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

        let environment_redeploy = environment.clone();
        let environment_check1 = environment.clone();
        let environment_check2 = environment.clone();
        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;

        let env_action = EnvironmentAction::Environment(environment.clone());
        let env_action_redeploy = EnvironmentAction::Environment(environment_redeploy);
        let env_action_delete = EnvironmentAction::Environment(environment_delete);

        match deploy_environment(&context, env_action) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        let app_name = format!("{}-0", &environment_check1.applications[0].name);
        let (_, number) = is_pod_restarted_env(
            Kind::Scw,
            SCW_TEST_CLUSTER_ID,
            environment_check1,
            app_name.clone().as_str(),
            secrets.clone(),
        );

        match deploy_environment(&context_bis, env_action_redeploy) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        let (_, number2) = is_pod_restarted_env(
            Kind::Scw,
            SCW_TEST_CLUSTER_ID,
            environment_check2,
            app_name.as_str(),
            secrets.clone(),
        );

        // nothing changed in the app, so, it shouldn't be restarted
        assert!(number.eq(&number2));

        match delete_environment(&context_for_deletion, env_action_delete) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        // delete container registries created during the test
        for app in environment.applications.iter() {
            assert_eq!(
                true,
                delete_container_registry(context.clone(), app.name.as_str(), secrets.clone()).is_ok()
            );
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
        init();

        let span = span!(Level::INFO, "test", name = test_name,);
        let _enter = span.enter();

        let context = context();
        let context_for_not_working = context.clone_not_same_execution_id();
        let context_for_delete = context.clone_not_same_execution_id();
        let secrets = FuncTestsSecrets::new();

        // env part generation
        let environment = test_utilities::scaleway::working_minimal_environment(&context, secrets.clone());
        let mut environment_for_not_working = environment.clone();
        // this environment is broken by container exit
        environment_for_not_working.applications = environment_for_not_working
            .applications
            .into_iter()
            .map(|mut app| {
                app.git_url = "https://github.com/Qovery/engine-testing.git".to_string();
                app.branch = "1app_fail_deploy".to_string();
                app.commit_id = "5b89305b9ae8a62a1f16c5c773cddf1d12f70db1".to_string();
                app.environment_variables = vec![];
                app
            })
            .collect::<Vec<qovery_engine::models::Application>>();

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // environment actions
        let env_action = EnvironmentAction::Environment(environment.clone());
        let env_action_not_working = EnvironmentAction::Environment(environment_for_not_working);
        let env_action_delete = EnvironmentAction::Environment(environment_for_delete);

        match deploy_environment(&context_for_not_working, env_action_not_working) {
            TransactionResult::Ok => assert!(false),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(true),
        };
        match deploy_environment(&context, env_action) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };
        match delete_environment(&context_for_delete, env_action_delete) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        // delete container registries created during the test
        for app in environment.applications.iter() {
            assert_eq!(
                true,
                delete_container_registry(context.clone(), app.name.as_str(), secrets.clone()).is_ok()
            );
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
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        // working env
        let context = context();
        let secrets = FuncTestsSecrets::new();
        let environment = test_utilities::scaleway::working_minimal_environment(&context, secrets.clone());

        // not working 1
        let context_for_not_working_1 = context.clone_not_same_execution_id();
        let mut not_working_env_1 = environment.clone();
        not_working_env_1.applications = not_working_env_1
            .applications
            .into_iter()
            .map(|mut app| {
                app.git_url = "https://gitlab.com/maathor/my-exit-container".to_string();
                app.branch = "master".to_string();
                app.commit_id = "55bc95a23fbf91a7699c28c5f61722d4f48201c9".to_string();
                app.environment_variables = vec![];
                app
            })
            .collect::<Vec<qovery_engine::models::Application>>();

        // not working 2
        let context_for_not_working_2 = context.clone_not_same_execution_id();
        let not_working_env_2 = not_working_env_1.clone();

        // work for delete
        let context_for_delete = context.clone_not_same_execution_id();
        let mut delete_env = environment.clone();
        delete_env.action = Action::Delete;

        let env_action = EnvironmentAction::Environment(environment.clone());
        let env_action_not_working_1 = EnvironmentAction::Environment(not_working_env_1);
        let env_action_not_working_2 = EnvironmentAction::Environment(not_working_env_2);
        let env_action_delete = EnvironmentAction::Environment(delete_env);

        // OK
        match deploy_environment(&context, env_action.clone()) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        // FAIL and rollback
        match deploy_environment(&context_for_not_working_1, env_action_not_working_1) {
            TransactionResult::Ok => assert!(false),
            TransactionResult::Rollback(_) => assert!(true),
            TransactionResult::UnrecoverableError(_, _) => assert!(true),
        };

        // FAIL and Rollback again
        match deploy_environment(&context_for_not_working_2, env_action_not_working_2) {
            TransactionResult::Ok => assert!(false),
            TransactionResult::Rollback(_) => assert!(true),
            TransactionResult::UnrecoverableError(_, _) => assert!(true),
        };

        // Should be working
        match deploy_environment(&context, env_action.clone()) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        match delete_environment(&context_for_delete, env_action_delete) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        // delete container registries created during the test
        for app in environment.applications.iter() {
            assert_eq!(
                true,
                delete_container_registry(context.clone(), app.name.as_str(), secrets.clone()).is_ok()
            );
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
        init();

        let span = span!(Level::INFO, "test", name = test_name,);
        let _enter = span.enter();

        let context = context();
        let secrets = FuncTestsSecrets::new();
        let environment = test_utilities::scaleway::non_working_environment(&context, secrets.clone());

        let context_for_delete = context.clone_not_same_execution_id();
        let mut delete_env = environment.clone();
        delete_env.action = Action::Delete;

        let env_action = EnvironmentAction::Environment(environment.clone());
        let env_action_delete = EnvironmentAction::Environment(delete_env);

        match deploy_environment(&context, env_action) {
            TransactionResult::Ok => assert!(false),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(true),
        };

        match delete_environment(&context_for_delete, env_action_delete) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        // delete container registries created during the test
        for app in environment.applications.iter() {
            assert_eq!(
                true,
                delete_container_registry(context.clone(), app.name.as_str(), secrets.clone()).is_ok()
            );
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn scaleway_kapsule_deploy_a_non_working_environment_with_a_working_failover() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name,);
        let _enter = span.enter();

        // context for non working environment
        let context = context();
        let secrets = FuncTestsSecrets::new();

        let environment = test_utilities::scaleway::non_working_environment(&context, secrets.clone());
        let failover_environment = test_utilities::scaleway::working_minimal_environment(&context, secrets.clone());

        // context for deletion
        let context_deletion = context.clone_not_same_execution_id();
        let mut delete_env = test_utilities::scaleway::working_minimal_environment(&context_deletion, secrets.clone());
        delete_env.action = Action::Delete;

        let env_action_delete = EnvironmentAction::Environment(delete_env);
        let env_action = EnvironmentAction::EnvironmentWithFailover(environment.clone(), failover_environment);

        match deploy_environment(&context, env_action) {
            TransactionResult::Ok => assert!(false),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(true),
        };

        match delete_environment(&context_deletion, env_action_delete) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        // delete container registries created during the test
        for app in environment.applications.iter() {
            assert_eq!(
                true,
                delete_container_registry(context.clone(), app.name.as_str(), secrets.clone()).is_ok()
            );
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn scaleway_kapsule_deploy_a_non_working_environment_with_a_non_working_failover() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name,);
        let _enter = span.enter();

        let context = context();
        let secrets = FuncTestsSecrets::new();

        let environment = test_utilities::scaleway::non_working_environment(&context, secrets.clone());
        let failover_environment = test_utilities::scaleway::non_working_environment(&context, secrets.clone());

        let context_for_deletion = context.clone_not_same_execution_id();
        let mut delete_env = test_utilities::scaleway::non_working_environment(&context_for_deletion, secrets.clone());
        delete_env.action = Action::Delete;

        // environment action initialize
        let env_action_delete = EnvironmentAction::Environment(delete_env);
        let env_action = EnvironmentAction::EnvironmentWithFailover(environment.clone(), failover_environment);

        match deploy_environment(&context, env_action) {
            TransactionResult::Ok => assert!(false),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(true),
        };

        match delete_environment(&context_for_deletion, env_action_delete) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(true),
        };

        // delete container registries created during the test
        for app in environment.applications.iter() {
            assert_eq!(
                true,
                delete_container_registry(context.clone(), app.name.as_str(), secrets.clone()).is_ok()
            );
        }

        test_name.to_string()
    })
}

// Note: All those tests relies on a test cluster running on DigitalOcean infrastructure.
// This cluster should be live in order to have those tests passing properly.

pub use crate::helpers::helpers_common::{non_working_environment, working_minimal_environment};
pub use crate::helpers::helpers_digitalocean::{
    clean_environments, delete_environment, deploy_environment, pause_environment, DO_KUBE_TEST_CLUSTER_ID,
    DO_QOVERY_ORGANIZATION_ID, DO_TEST_REGION,
};
pub use crate::helpers::utilities::{
    context, engine_run_test, generate_id, get_pods, init, is_pod_restarted_env, FuncTestsSecrets,
};
pub use function_name::named;
pub use qovery_engine::cloud_provider::Kind;
pub use qovery_engine::models::{Action, Clone2, EnvironmentAction, Storage, StorageType};
pub use qovery_engine::transaction::TransactionResult;
pub use std::collections::BTreeMap;
pub use tracing::{span, warn, Level};

#[cfg(feature = "test-do-self-hosted")]
#[ignore]
#[named]
#[test]
fn digitalocean_doks_deploy_a_working_environment_with_no_router() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let context = context();
        let context_for_delete = context.clone_not_same_execution_id();
        let secrets = FuncTestsSecrets::new();
        let mut environment = working_minimal_environment(
            &context,
            DO_QOVERY_ORGANIZATION_ID,
            secrets
                .DEFAULT_TEST_DOMAIN
                .as_ref()
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

        let mut environment_for_delete = environment.clone();
        environment.routers = vec![];
        environment_for_delete.routers = vec![];
        environment_for_delete.action = Action::Delete;

        let env_action = EnvironmentAction::Environment(environment.clone());
        let env_action_for_delete = EnvironmentAction::Environment(environment_for_delete);

        match deploy_environment(&context, env_action, DO_TEST_REGION) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        match delete_environment(&context_for_delete, env_action_for_delete, DO_TEST_REGION) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        if let Err(e) = clean_environments(&context, vec![environment], secrets, DO_TEST_REGION) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-do-self-hosted")]
#[ignore]
#[named]
#[test]
fn digitalocean_doks_deploy_a_not_working_environment_with_no_router() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name,);
        let _enter = span.enter();

        let context = context();
        let context_for_delete = context.clone_not_same_execution_id();
        let secrets = FuncTestsSecrets::new();

        let mut environment = non_working_environment(
            &context,
            DO_QOVERY_ORGANIZATION_ID,
            secrets
                .DEFAULT_TEST_DOMAIN
                .as_ref()
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );
        environment.routers = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let env_action = EnvironmentAction::Environment(environment.clone());
        let env_action_for_delete = EnvironmentAction::Environment(environment_for_delete);

        match deploy_environment(&context, env_action, DO_TEST_REGION) {
            TransactionResult::Ok => panic!(),
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => {}
        };

        match delete_environment(&context_for_delete, env_action_for_delete, DO_TEST_REGION) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => {}
        };

        if let Err(e) = clean_environments(&context, vec![environment], secrets, DO_TEST_REGION) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-do-self-hosted")]
#[ignore]
#[named]
#[test]
fn digitalocean_doks_deploy_a_working_environment_and_pause() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let context = context();
        let context_for_delete = context.clone_not_same_execution_id();
        let secrets = FuncTestsSecrets::new();
        let environment = working_minimal_environment(
            &context,
            DO_QOVERY_ORGANIZATION_ID,
            secrets
                .DEFAULT_TEST_DOMAIN
                .as_ref()
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

        let env_action = EnvironmentAction::Environment(environment.clone());

        match deploy_environment(&context, env_action.clone(), DO_TEST_REGION) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        match pause_environment(&context_for_delete, env_action.clone(), DO_TEST_REGION) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        // Check that we have actually 0 pods running for this app
        let app_name = format!("{}-0", environment.applications[0].name);
        let ret = get_pods(
            Kind::Do,
            environment.clone(),
            app_name.as_str(),
            DO_KUBE_TEST_CLUSTER_ID,
            secrets.clone(),
        );
        assert!(ret.is_ok());
        assert!(ret.unwrap().items.is_empty());

        // Check we can resume the env
        let ctx_resume = context.clone_not_same_execution_id();
        match deploy_environment(&ctx_resume, env_action.clone(), DO_TEST_REGION) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        // Cleanup
        match delete_environment(&context_for_delete, env_action, DO_TEST_REGION) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        if let Err(e) = clean_environments(&context, vec![environment], secrets, DO_TEST_REGION) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-do-self-hosted")]
#[ignore]
#[named]
#[test]
fn digitalocean_doks_build_with_buildpacks_and_deploy_a_working_environment() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name,);
        let _enter = span.enter();

        let context = context();
        let context_for_delete = context.clone_not_same_execution_id();
        let secrets = FuncTestsSecrets::new();
        let mut environment = working_minimal_environment(
            &context,
            DO_QOVERY_ORGANIZATION_ID,
            secrets
                .DEFAULT_TEST_DOMAIN
                .as_ref()
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );
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

        match deploy_environment(&context, env_action, DO_TEST_REGION) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        match delete_environment(&context_for_delete, env_action_for_delete, DO_TEST_REGION) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        if let Err(e) = clean_environments(&context, vec![environment], secrets, DO_TEST_REGION) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-do-self-hosted")]
#[ignore]
#[named]
#[test]
fn digitalocean_doks_deploy_a_working_environment_with_domain() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test",);
        let _enter = span.enter();

        let context = context();
        let context_for_delete = context.clone_not_same_execution_id();
        let secrets = FuncTestsSecrets::new();
        let environment = working_minimal_environment(
            &context,
            DO_QOVERY_ORGANIZATION_ID,
            secrets
                .DEFAULT_TEST_DOMAIN
                .as_ref()
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;

        let env_action = EnvironmentAction::Environment(environment.clone());
        let env_action_for_delete = EnvironmentAction::Environment(environment_delete);

        match deploy_environment(&context, env_action, DO_TEST_REGION) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        match delete_environment(&context_for_delete, env_action_for_delete, DO_TEST_REGION) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        if let Err(e) = clean_environments(&context, vec![environment], secrets, DO_TEST_REGION) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-do-self-hosted")]
#[ignore]
#[named]
#[test]
fn digitalocean_doks_deploy_a_working_environment_with_storage() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name,);
        let _enter = span.enter();

        let context = context();
        let context_for_deletion = context.clone_not_same_execution_id();
        let secrets = FuncTestsSecrets::new();

        let mut environment = working_minimal_environment(
            &context,
            DO_QOVERY_ORGANIZATION_ID,
            secrets
                .DEFAULT_TEST_DOMAIN
                .as_ref()
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

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

        match deploy_environment(&context, env_action, DO_TEST_REGION) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        // TODO(benjaminch): check the disk is here and with correct size, can use DigitalOcean API

        match delete_environment(&context_for_deletion, env_action_delete, DO_TEST_REGION) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        if let Err(e) = clean_environments(&context, vec![environment], secrets, DO_TEST_REGION) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-do-self-hosted")]
#[ignore]
#[named]
#[test]
fn digitalocean_doks_redeploy_same_app() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let context = context();
        let context_bis = context.clone_not_same_execution_id();
        let context_for_deletion = context.clone_not_same_execution_id();
        let secrets = FuncTestsSecrets::new();

        let mut environment = working_minimal_environment(
            &context,
            DO_QOVERY_ORGANIZATION_ID,
            secrets
                .DEFAULT_TEST_DOMAIN
                .as_ref()
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

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

        match deploy_environment(&context, env_action, DO_TEST_REGION) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        let app_name = format!("{}-0", &environment_check1.applications[0].name);
        let (_, number) = is_pod_restarted_env(
            Kind::Do,
            DO_KUBE_TEST_CLUSTER_ID,
            environment_check1,
            app_name.as_str(),
            secrets.clone(),
        );

        match deploy_environment(&context_bis, env_action_redeploy, DO_TEST_REGION) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        let (_, number2) = is_pod_restarted_env(
            Kind::Do,
            DO_KUBE_TEST_CLUSTER_ID,
            environment_check2,
            app_name.as_str(),
            secrets.clone(),
        );

        // nothing changed in the app, so, it shouldn't be restarted
        assert!(number.eq(&number2));

        match delete_environment(&context_for_deletion, env_action_delete, DO_TEST_REGION) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        if let Err(e) = clean_environments(&context, vec![environment], secrets, DO_TEST_REGION) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-do-self-hosted")]
#[ignore]
#[named]
#[test]
fn digitalocean_doks_deploy_a_not_working_environment_and_then_working_environment() {
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
        let environment = working_minimal_environment(
            &context,
            DO_QOVERY_ORGANIZATION_ID,
            secrets
                .DEFAULT_TEST_DOMAIN
                .as_ref()
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );
        let mut environment_for_not_working = environment.clone();
        // this environment is broken by container exit
        environment_for_not_working.applications = environment_for_not_working
            .applications
            .into_iter()
            .map(|mut app| {
                app.git_url = "https://github.com/Qovery/engine-testing.git".to_string();
                app.branch = "1app_fail_deploy".to_string();
                app.commit_id = "5b89305b9ae8a62a1f16c5c773cddf1d12f70db1".to_string();
                app.environment_vars = BTreeMap::new();
                app
            })
            .collect::<Vec<qovery_engine::models::Application>>();

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // environment actions
        let env_action = EnvironmentAction::Environment(environment.clone());
        let env_action_not_working = EnvironmentAction::Environment(environment_for_not_working);
        let env_action_delete = EnvironmentAction::Environment(environment_for_delete);

        match deploy_environment(&context_for_not_working, env_action_not_working, DO_TEST_REGION) {
            TransactionResult::Ok => panic!(),
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => {}
        };
        match deploy_environment(&context, env_action, DO_TEST_REGION) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };
        match delete_environment(&context_for_delete, env_action_delete, DO_TEST_REGION) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        if let Err(e) = clean_environments(&context, vec![environment], secrets, DO_TEST_REGION) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
#[ignore] // TODO(benjaminch): Make it work (it doesn't work on AWS neither)
fn digitalocean_doks_deploy_ok_fail_fail_ok_environment() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        // working env
        let context = context();
        let secrets = FuncTestsSecrets::new();
        let environment = working_minimal_environment(
            &context,
            DO_QOVERY_ORGANIZATION_ID,
            secrets
                .DEFAULT_TEST_DOMAIN
                .as_ref()
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

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
                app.environment_vars = BTreeMap::new();
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
        match deploy_environment(&context, env_action.clone(), DO_TEST_REGION) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        // FAIL and rollback
        match deploy_environment(&context_for_not_working_1, env_action_not_working_1, DO_TEST_REGION) {
            TransactionResult::Ok => panic!(),
            TransactionResult::Rollback(_) => {}
            TransactionResult::UnrecoverableError(_, _) => {}
        };

        // FAIL and Rollback again
        match deploy_environment(&context_for_not_working_2, env_action_not_working_2, DO_TEST_REGION) {
            TransactionResult::Ok => panic!(),
            TransactionResult::Rollback(_) => {}
            TransactionResult::UnrecoverableError(_, _) => {}
        };

        // Should be working
        match deploy_environment(&context, env_action, DO_TEST_REGION) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        match delete_environment(&context_for_delete, env_action_delete, DO_TEST_REGION) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        if let Err(e) = clean_environments(&context, vec![environment], secrets, DO_TEST_REGION) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-do-self-hosted")]
#[ignore]
#[named]
#[test]
fn digitalocean_doks_deploy_a_non_working_environment_with_no_failover() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name,);
        let _enter = span.enter();

        let context = context();
        let secrets = FuncTestsSecrets::new();
        let environment = non_working_environment(
            &context,
            DO_QOVERY_ORGANIZATION_ID,
            secrets
                .DEFAULT_TEST_DOMAIN
                .as_ref()
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

        let context_for_delete = context.clone_not_same_execution_id();
        let mut delete_env = environment.clone();
        delete_env.action = Action::Delete;

        let env_action = EnvironmentAction::Environment(environment.clone());
        let env_action_delete = EnvironmentAction::Environment(delete_env);

        match deploy_environment(&context, env_action, DO_TEST_REGION) {
            TransactionResult::Ok => panic!(),
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => {}
        };

        match delete_environment(&context_for_delete, env_action_delete, DO_TEST_REGION) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        if let Err(e) = clean_environments(&context, vec![environment], secrets, DO_TEST_REGION) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-do-self-hosted")]
#[ignore]
#[named]
#[test]
fn digitalocean_doks_deploy_a_non_working_environment_with_a_working_failover() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name,);
        let _enter = span.enter();

        // context for non working environment
        let context = context();
        let secrets = FuncTestsSecrets::new();
        let test_domain = secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets");

        let environment = non_working_environment(&context, DO_QOVERY_ORGANIZATION_ID, test_domain.as_str());
        let failover_environment =
            working_minimal_environment(&context, DO_QOVERY_ORGANIZATION_ID, test_domain.as_str());

        // context for deletion
        let context_deletion = context.clone_not_same_execution_id();
        let mut delete_env =
            working_minimal_environment(&context_deletion, DO_QOVERY_ORGANIZATION_ID, test_domain.as_str());
        delete_env.action = Action::Delete;

        let env_action_delete = EnvironmentAction::Environment(delete_env);
        let env_action =
            EnvironmentAction::EnvironmentWithFailover(environment.clone(), Box::new(failover_environment.clone()));

        match deploy_environment(&context, env_action, DO_TEST_REGION) {
            TransactionResult::Ok => panic!(),
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => {}
        };

        match delete_environment(&context_deletion, env_action_delete, DO_TEST_REGION) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        if let Err(e) = clean_environments(
            &context,
            vec![environment, failover_environment],
            secrets.clone(),
            DO_TEST_REGION,
        ) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-do-self-hosted")]
#[ignore]
#[named]
#[test]
fn digitalocean_doks_deploy_a_non_working_environment_with_a_non_working_failover() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name,);
        let _enter = span.enter();

        let context = context();
        let secrets = FuncTestsSecrets::new();
        let test_domain = secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets");

        let environment = non_working_environment(&context, DO_QOVERY_ORGANIZATION_ID, test_domain.as_str());
        let failover_environment = non_working_environment(&context, DO_QOVERY_ORGANIZATION_ID, test_domain.as_str());

        let context_for_deletion = context.clone_not_same_execution_id();
        let mut delete_env =
            non_working_environment(&context_for_deletion, DO_QOVERY_ORGANIZATION_ID, test_domain.as_str());
        delete_env.action = Action::Delete;

        // environment action initialize
        let env_action_delete = EnvironmentAction::Environment(delete_env);
        let env_action =
            EnvironmentAction::EnvironmentWithFailover(environment.clone(), Box::new(failover_environment.clone()));

        match deploy_environment(&context, env_action, DO_TEST_REGION) {
            TransactionResult::Ok => panic!(),
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => {}
        };

        match delete_environment(&context_for_deletion, env_action_delete, DO_TEST_REGION) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => {}
        };

        if let Err(e) = clean_environments(
            &context,
            vec![environment, failover_environment],
            secrets.clone(),
            DO_TEST_REGION,
        ) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

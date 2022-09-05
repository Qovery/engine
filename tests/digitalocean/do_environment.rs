use crate::helpers;
use crate::helpers::common::Infrastructure;
use crate::helpers::digitalocean::do_default_engine_config;
use crate::helpers::digitalocean::{clean_environments, DO_TEST_REGION};
use crate::helpers::environment::session_is_sticky;
use crate::helpers::utilities::{context, generate_id, get_pvc, is_pod_restarted_env};
use crate::helpers::utilities::{engine_run_test, get_pods, init, kubernetes_config_path, logger, FuncTestsSecrets};
use ::function_name::named;
use qovery_engine::cloud_provider::Kind;
use qovery_engine::io_models::application::{Port, Protocol, Storage, StorageType};
use qovery_engine::io_models::context::CloneForTest;
use qovery_engine::io_models::Action;
use qovery_engine::transaction::TransactionResult;
use qovery_engine::utilities::to_short_id;
use retry::delay::Fibonacci;
use std::collections::BTreeMap;
use tracing::{span, warn, Level};
use url::Url;
use uuid::Uuid;
// Note: All those tests relies on a test cluster running on DigitalOcean infrastructure.
// This cluster should be live in order to have those tests passing properly.

#[cfg(feature = "test-do-minimal")]
#[named]
#[test]
#[ignore]
fn digitalocean_test_build_phase() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let secrets = FuncTestsSecrets::new();
        let logger = logger();
        let context = context(
            secrets
                .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
            secrets
                .DIGITAL_OCEAN_TEST_CLUSTER_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_CLUSTER_ID is not set"),
        );
        let engine_config = do_default_engine_config(&context, logger.clone());
        let environment = helpers::environment::working_minimal_environment(&context);

        let env_action = environment.clone();

        let (env, ret) = environment.build_environment(&env_action, logger.clone(), &engine_config);
        assert!(matches!(ret, TransactionResult::Ok));

        // Check the the image exist in the registry
        let img_exist = engine_config
            .container_registry()
            .does_image_exists(&env.applications[0].get_build().image);
        assert!(img_exist);

        test_name.to_string()
    })
}

#[cfg(feature = "test-do-minimal")]
#[named]
#[test]
#[ignore]
fn digitalocean_doks_deploy_a_working_environment_with_no_router() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let secrets = FuncTestsSecrets::new();
        let logger = logger();
        let context = context(
            secrets
                .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
            secrets
                .DIGITAL_OCEAN_TEST_CLUSTER_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_CLUSTER_ID is not set"),
        );
        let engine_config = do_default_engine_config(&context, logger.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let engine_config_for_delete = do_default_engine_config(&context_for_delete, logger.clone());
        let environment = helpers::environment::working_minimal_environment(&context);

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let env_action = environment.clone();
        let env_action_for_delete = environment_for_delete.clone();

        let ret = environment.deploy_environment(&env_action, logger.clone(), &engine_config);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = environment_for_delete.delete_environment(&env_action_for_delete, logger, &engine_config_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        if let Err(e) = clean_environments(&context, vec![environment], secrets, DO_TEST_REGION) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
fn digitalocean_doks_deploy_a_not_working_environment_with_no_router() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name,);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context(
            secrets
                .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
            secrets
                .DIGITAL_OCEAN_TEST_CLUSTER_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_CLUSTER_ID is not set"),
        );
        let engine_config = do_default_engine_config(&context, logger.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let engine_config_for_delete = do_default_engine_config(&context_for_delete, logger.clone());

        let mut environment = helpers::environment::non_working_environment(&context);
        environment.routers = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let env_action = environment.clone();
        let env_action_for_delete = environment_for_delete.clone();

        let result = environment.deploy_environment(&env_action, logger.clone(), &engine_config);
        assert!(matches!(result, TransactionResult::Error(_)));

        let result =
            environment_for_delete.delete_environment(&env_action_for_delete, logger, &engine_config_for_delete);
        assert!(matches!(result, TransactionResult::Ok | TransactionResult::Error(_)));

        if let Err(e) = clean_environments(&context, vec![environment], secrets, DO_TEST_REGION) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
fn digitalocean_doks_deploy_a_working_environment_and_pause() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context(
            secrets
                .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
            secrets
                .DIGITAL_OCEAN_TEST_CLUSTER_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_CLUSTER_ID is not set"),
        );
        let engine_config = do_default_engine_config(&context, logger.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let engine_config_for_delete = do_default_engine_config(&context_for_delete, logger.clone());
        let environment = helpers::environment::working_minimal_environment(&context);

        let env_action = environment.clone();
        let selector = format!("appId={}", to_short_id(&environment.applications[0].long_id));

        let ret = environment.deploy_environment(&env_action, logger.clone(), &engine_config);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = get_pods(
            context.clone(),
            Kind::Do,
            environment.clone(),
            selector.as_str(),
            secrets.clone(),
        );
        assert!(ret.is_ok());
        assert!(!ret.unwrap().items.is_empty());

        let ret = environment.pause_environment(&env_action, logger.clone(), &engine_config_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        // Check that we have actually 0 pods running for this app
        let ret = get_pods(
            context.clone(),
            Kind::Do,
            environment.clone(),
            selector.as_str(),
            secrets.clone(),
        );
        assert!(ret.is_ok());
        assert!(ret.unwrap().items.is_empty());

        // Check we can resume the env
        let ctx_resume = context.clone_not_same_execution_id();
        let engine_config_resume = do_default_engine_config(&ctx_resume, logger.clone());
        let ret = environment.deploy_environment(&env_action, logger.clone(), &engine_config_resume);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = get_pods(
            context.clone(),
            Kind::Do,
            environment.clone(),
            selector.as_str(),
            secrets.clone(),
        );
        assert!(ret.is_ok());
        assert!(!ret.unwrap().items.is_empty());

        // Cleanup
        let ret = environment.delete_environment(&env_action, logger, &engine_config_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        if let Err(e) = clean_environments(&context, vec![environment], secrets, DO_TEST_REGION) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
fn digitalocean_doks_build_with_buildpacks_and_deploy_a_working_environment() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name,);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context(
            secrets
                .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
            secrets
                .DIGITAL_OCEAN_TEST_CLUSTER_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_CLUSTER_ID is not set"),
        );
        let engine_config = do_default_engine_config(&context, logger.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let engine_config_for_delete = do_default_engine_config(&context_for_delete, logger.clone());
        let mut environment = helpers::environment::working_minimal_environment(&context);
        environment.applications = environment
            .applications
            .into_iter()
            .map(|mut app| {
                app.ports = vec![Port {
                    id: "zdf7d6aad".to_string(),
                    long_id: Default::default(),
                    port: 3000,
                    public_port: Some(443),
                    name: None,
                    publicly_accessible: true,
                    protocol: Protocol::HTTP,
                }];
                app.commit_id = "f59237d603829636138e2f22a0549e33b5dd6e1f".to_string();
                app.branch = "simple-node-app".to_string();
                app.dockerfile_path = None;
                app
            })
            .collect::<Vec<qovery_engine::io_models::application::Application>>();

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let env_action = environment.clone();
        let env_action_for_delete = environment_for_delete.clone();

        let result = environment.deploy_environment(&env_action, logger.clone(), &engine_config);
        assert!(matches!(result, TransactionResult::Ok));

        let result =
            environment_for_delete.delete_environment(&env_action_for_delete, logger, &engine_config_for_delete);
        assert!(matches!(result, TransactionResult::Ok));

        if let Err(e) = clean_environments(&context, vec![environment], secrets, DO_TEST_REGION) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
fn digitalocean_doks_deploy_a_working_environment_with_domain() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test",);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context(
            secrets
                .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
            secrets
                .DIGITAL_OCEAN_TEST_CLUSTER_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_CLUSTER_ID is not set"),
        );
        let engine_config = do_default_engine_config(&context, logger.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let engine_config_for_delete = do_default_engine_config(&context_for_delete, logger.clone());
        let environment = helpers::environment::working_minimal_environment(&context);

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;

        let env_action = environment.clone();
        let env_action_for_delete = environment_delete.clone();

        let result = environment.deploy_environment(&env_action, logger.clone(), &engine_config);
        assert!(matches!(result, TransactionResult::Ok));

        let result = environment_delete.delete_environment(&env_action_for_delete, logger, &engine_config_for_delete);
        assert!(matches!(result, TransactionResult::Ok));

        if let Err(e) = clean_environments(&context, vec![environment], secrets, DO_TEST_REGION) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
fn digitalocean_doks_deploy_a_working_environment_with_storage() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name,);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context(
            secrets
                .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
            secrets
                .DIGITAL_OCEAN_TEST_CLUSTER_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_CLUSTER_ID is not set"),
        );
        let engine_config = do_default_engine_config(&context, logger.clone());
        let context_for_deletion = context.clone_not_same_execution_id();
        let engine_config_for_deletion = do_default_engine_config(&context_for_deletion, logger.clone());
        let mut environment = helpers::environment::working_minimal_environment(&context);

        let storage_size: u16 = 10;
        environment.applications = environment
            .applications
            .into_iter()
            .map(|mut app| {
                app.storage = vec![Storage {
                    id: generate_id(),
                    long_id: Uuid::new_v4(),
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

        let env_action = environment.clone();
        let env_action_delete = environment_delete.clone();

        let result = environment.deploy_environment(&env_action, logger.clone(), &engine_config);
        assert!(matches!(result, TransactionResult::Ok));

        match get_pvc(context.clone(), Kind::Do, environment.clone(), secrets.clone()) {
            Ok(pvc) => assert_eq!(
                pvc.items.expect("No items in pvc")[0].spec.resources.requests.storage,
                format!("{}Gi", storage_size)
            ),
            Err(_) => panic!(),
        };

        let result = environment_delete.delete_environment(&env_action_delete, logger, &engine_config_for_deletion);
        assert!(matches!(result, TransactionResult::Ok));

        if let Err(e) = clean_environments(&context, vec![environment], secrets, DO_TEST_REGION) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
fn digitalocean_doks_redeploy_same_app() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context(
            secrets
                .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
            secrets
                .DIGITAL_OCEAN_TEST_CLUSTER_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_CLUSTER_ID is not set"),
        );
        let engine_config = do_default_engine_config(&context, logger.clone());
        let context_bis = context.clone_not_same_execution_id();
        let engine_config_bis = do_default_engine_config(&context_bis, logger.clone());
        let context_for_deletion = context.clone_not_same_execution_id();
        let engine_config_for_deletion = do_default_engine_config(&context_for_deletion, logger.clone());

        let mut environment = helpers::environment::working_minimal_environment(&context);

        let storage_size: u16 = 10;
        environment.applications = environment
            .applications
            .into_iter()
            .map(|mut app| {
                app.storage = vec![Storage {
                    id: generate_id(),
                    long_id: Uuid::new_v4(),
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

        let env_action = environment.clone();
        let env_action_redeploy = environment_redeploy.clone();
        let env_action_delete = environment_delete.clone();

        let result = environment.deploy_environment(&env_action, logger.clone(), &engine_config);
        assert!(matches!(result, TransactionResult::Ok));

        match get_pvc(context.clone(), Kind::Do, environment.clone(), secrets.clone()) {
            Ok(pvc) => assert_eq!(
                pvc.items.expect("No items in pvc")[0].spec.resources.requests.storage,
                format!("{}Gi", storage_size)
            ),
            Err(_) => panic!(),
        };

        let app_name = format!("{}-0", &environment_check1.applications[0].name);
        let (_, number) = is_pod_restarted_env(
            context.clone(),
            Kind::Do,
            environment_check1,
            app_name.as_str(),
            secrets.clone(),
        );

        let result = environment_redeploy.deploy_environment(&env_action_redeploy, logger.clone(), &engine_config_bis);
        assert!(matches!(result, TransactionResult::Ok));

        let (_, number2) = is_pod_restarted_env(
            context.clone(),
            Kind::Do,
            environment_check2,
            app_name.as_str(),
            secrets.clone(),
        );

        // nothing changed in the app, so, it shouldn't be restarted
        assert!(number.eq(&number2));

        let result = environment_delete.delete_environment(&env_action_delete, logger, &engine_config_for_deletion);
        assert!(matches!(result, TransactionResult::Ok));

        if let Err(e) = clean_environments(&context, vec![environment], secrets, DO_TEST_REGION) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
fn digitalocean_doks_deploy_a_not_working_environment_and_then_working_environment() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name,);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context(
            secrets
                .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
            secrets
                .DIGITAL_OCEAN_TEST_CLUSTER_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_CLUSTER_ID is not set"),
        );
        let engine_config = do_default_engine_config(&context, logger.clone());
        let context_for_not_working = context.clone_not_same_execution_id();
        let engine_config_for_not_working = do_default_engine_config(&context_for_not_working, logger.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let engine_config_for_delete = do_default_engine_config(&context_for_delete, logger.clone());

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
                app.environment_vars = BTreeMap::new();
                app
            })
            .collect::<Vec<qovery_engine::io_models::application::Application>>();

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // environment actions
        let env_action = environment.clone();
        let env_action_not_working = environment_for_not_working.clone();
        let env_action_delete = environment_for_delete.clone();

        let result = environment_for_not_working.deploy_environment(
            &env_action_not_working,
            logger.clone(),
            &engine_config_for_not_working,
        );
        assert!(matches!(result, TransactionResult::Error(_)));
        let result = environment.deploy_environment(&env_action, logger.clone(), &engine_config);
        assert!(matches!(result, TransactionResult::Ok));
        let result = environment_for_delete.delete_environment(&env_action_delete, logger, &engine_config_for_delete);
        assert!(matches!(result, TransactionResult::Ok));

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

        let logger = logger();
        let secrets = FuncTestsSecrets::new();

        // working env
        let context = context(
            secrets
                .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
            secrets
                .DIGITAL_OCEAN_TEST_CLUSTER_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_CLUSTER_ID is not set"),
        );
        let engine_config = do_default_engine_config(&context, logger.clone());
        let environment = helpers::environment::working_minimal_environment(&context);

        // not working 1
        let context_for_not_working_1 = context.clone_not_same_execution_id();
        let engine_config_for_not_working_1 = do_default_engine_config(&context_for_not_working_1, logger.clone());
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
            .collect::<Vec<qovery_engine::io_models::application::Application>>();

        // not working 2
        let context_for_not_working_2 = context.clone_not_same_execution_id();
        let engine_config_for_not_working_2 = do_default_engine_config(&context_for_not_working_2, logger.clone());
        let not_working_env_2 = not_working_env_1.clone();

        // work for delete
        let context_for_delete = context.clone_not_same_execution_id();
        let engine_config_for_delete = do_default_engine_config(&context_for_delete, logger.clone());
        let mut delete_env = environment.clone();
        delete_env.action = Action::Delete;

        let env_action = environment.clone();
        let env_action_not_working_1 = not_working_env_1.clone();
        let env_action_not_working_2 = not_working_env_2.clone();
        let env_action_delete = delete_env.clone();

        // OK
        let result = environment.deploy_environment(&env_action, logger.clone(), &engine_config);
        assert!(matches!(result, TransactionResult::Ok));

        // FAIL and rollback
        let result = not_working_env_1.deploy_environment(
            &env_action_not_working_1,
            logger.clone(),
            &engine_config_for_not_working_1,
        );
        assert!(matches!(result, TransactionResult::Error(_)));

        // FAIL and Rollback again
        let result = not_working_env_2.deploy_environment(
            &env_action_not_working_2,
            logger.clone(),
            &engine_config_for_not_working_2,
        );
        assert!(matches!(result, TransactionResult::Error(_)));

        // Should be working
        let result = environment.deploy_environment(&env_action, logger.clone(), &engine_config);
        assert!(matches!(result, TransactionResult::Ok));

        let result = delete_env.delete_environment(&env_action_delete, logger, &engine_config_for_delete);
        assert!(matches!(result, TransactionResult::Ok));

        if let Err(e) = clean_environments(&context, vec![environment], secrets, DO_TEST_REGION) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
fn digitalocean_doks_deploy_a_non_working_environment_with_no_failover() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name,);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context(
            secrets
                .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
            secrets
                .DIGITAL_OCEAN_TEST_CLUSTER_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_CLUSTER_ID is not set"),
        );
        let engine_config = do_default_engine_config(&context, logger.clone());
        let environment = helpers::environment::non_working_environment(&context);

        let context_for_delete = context.clone_not_same_execution_id();
        let engine_config_for_delete = do_default_engine_config(&context_for_delete, logger.clone());
        let mut delete_env = environment.clone();
        delete_env.action = Action::Delete;

        let env_action = environment.clone();
        let env_action_delete = delete_env.clone();

        let result = environment.deploy_environment(&env_action, logger.clone(), &engine_config);
        assert!(matches!(result, TransactionResult::Error(_)));

        let result = delete_env.delete_environment(&env_action_delete, logger, &engine_config_for_delete);
        assert!(matches!(result, TransactionResult::Ok));

        if let Err(e) = clean_environments(&context, vec![environment], secrets, DO_TEST_REGION) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
fn digitalocean_doks_deploy_a_working_environment_with_sticky_session() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context(
            secrets
                .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID in secrets")
                .as_str(),
            secrets
                .DIGITAL_OCEAN_TEST_CLUSTER_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_CLUSTER_ID is not set in secrets")
                .as_str(),
        );
        let engine_config = do_default_engine_config(&context, logger.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let engine_config_for_delete = do_default_engine_config(&context_for_delete, logger.clone());
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

        let result = environment.deploy_environment(&env_action, logger.clone(), &engine_config);
        assert!(matches!(result, TransactionResult::Ok));

        // checking cookie is properly set on the app
        let kubeconfig = kubernetes_config_path(engine_config.context().clone(), Kind::Do, "/tmp", secrets.clone())
            .expect("cannot get kubeconfig");
        let router = environment
            .routers
            .first()
            .unwrap()
            .to_router_domain(
                engine_config.context(),
                true,
                "0.0.0.0/0".to_string(),
                engine_config.cloud_provider(),
                logger.clone(),
            )
            .unwrap();
        let environment_domain = environment
            .to_environment_domain(
                engine_config.context(),
                engine_config.cloud_provider(),
                engine_config.container_registry(),
                logger.clone(),
            )
            .unwrap();

        // let some time for ingress to get its IP or hostname
        // Sticky session is checked on ingress IP or hostname so we are not subjects to long DNS propagation making test less flacky.
        let ingress = retry::retry(Fibonacci::from_millis(15000).take(8), || {
            match qovery_engine::cmd::kubectl::kubectl_exec_get_external_ingress(
                &kubeconfig,
                environment_domain.namespace(),
                router.sanitized_name().as_str(),
                engine_config.cloud_provider().credentials_environment_variables(),
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

        let result =
            environment_for_delete.delete_environment(&env_action_for_delete, logger, &engine_config_for_delete);
        assert!(matches!(result, TransactionResult::Ok));

        if let Err(e) = clean_environments(&context, vec![environment], secrets, DO_TEST_REGION) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

extern crate test_utilities;

use self::test_utilities::common::routers_sessions_are_sticky;
use self::test_utilities::scaleway::{clean_environments, SCW_TEST_ZONE};
use self::test_utilities::utilities::{
    context, engine_run_test, generate_id, get_pods, get_pvc, init, is_pod_restarted_env, logger, FuncTestsSecrets,
};
use ::function_name::named;
use qovery_engine::cloud_provider::Kind;
use qovery_engine::models::{Action, CloneForTest, Port, Protocol, Storage, StorageType};
use qovery_engine::transaction::TransactionResult;
use std::collections::BTreeMap;
use std::thread;
use std::time::Duration;
use test_utilities::common::Infrastructure;
use test_utilities::scaleway::scw_default_engine_config;
use tracing::{span, warn, Level};

// Note: All those tests relies on a test cluster running on Scaleway infrastructure.
// This cluster should be live in order to have those tests passing properly.

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn scaleway_test_build_phase() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("SCALEWAY_TEST_ORGANIZATION_ID")
                .as_str(),
            secrets
                .SCALEWAY_TEST_CLUSTER_ID
                .as_ref()
                .expect("SCALEWAY_TEST_CLUSTER_ID")
                .as_str(),
        );
        let engine_config = scw_default_engine_config(&context, logger.clone());
        let environment = test_utilities::common::working_minimal_environment(
            &context,
            secrets
                .DEFAULT_TEST_DOMAIN
                .as_ref()
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

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

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn scaleway_kapsule_deploy_a_working_environment_with_no_router() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("SCALEWAY_TEST_ORGANIZATION_ID")
                .as_str(),
            secrets
                .SCALEWAY_TEST_CLUSTER_ID
                .as_ref()
                .expect("SCALEWAY_TEST_CLUSTER_ID")
                .as_str(),
        );
        let engine_config = scw_default_engine_config(&context, logger.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let engine_config_for_delete = scw_default_engine_config(&context_for_delete, logger.clone());
        let mut environment = test_utilities::common::working_minimal_environment(
            &context,
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

        let env_action = environment.clone();
        let env_action_for_delete = environment_for_delete.clone();

        let result = environment.deploy_environment(&env_action, logger.clone(), &engine_config);
        assert!(matches!(result, TransactionResult::Ok));

        let result =
            environment_for_delete.delete_environment(&env_action_for_delete, logger, &engine_config_for_delete);
        assert!(matches!(result, TransactionResult::Ok));

        if let Err(e) = clean_environments(&context, vec![environment], secrets, SCW_TEST_ZONE) {
            warn!("cannot clean environments, error: {:?}", e);
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

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("SCALEWAY_TEST_ORGANIZATION_ID")
                .as_str(),
            secrets
                .SCALEWAY_TEST_CLUSTER_ID
                .as_ref()
                .expect("SCALEWAY_TEST_CLUSTER_ID")
                .as_str(),
        );
        let engine_config = scw_default_engine_config(&context, logger.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let engine_config_for_delete = scw_default_engine_config(&context_for_delete, logger.clone());

        let mut environment = test_utilities::common::non_working_environment(
            &context,
            secrets
                .DEFAULT_TEST_DOMAIN
                .as_ref()
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );
        environment.routers = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let env_action = environment.clone();
        let env_action_for_delete = environment_for_delete.clone();

        let result = environment.deploy_environment(&env_action, logger.clone(), &engine_config);
        assert!(matches!(result, TransactionResult::UnrecoverableError(_, _)));

        let result =
            environment_for_delete.delete_environment(&env_action_for_delete, logger, &engine_config_for_delete);
        assert!(matches!(
            result,
            TransactionResult::Ok | TransactionResult::UnrecoverableError(_, _)
        ));

        if let Err(e) = clean_environments(&context, vec![environment], secrets, SCW_TEST_ZONE) {
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
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("SCALEWAY_TEST_ORGANIZATION_ID")
                .as_str(),
            secrets
                .SCALEWAY_TEST_CLUSTER_ID
                .as_ref()
                .expect("SCALEWAY_TEST_CLUSTER_ID")
                .as_str(),
        );
        let engine_config = scw_default_engine_config(&context, logger.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let engine_config_for_delete = scw_default_engine_config(&context_for_delete, logger.clone());
        let environment = test_utilities::common::working_minimal_environment(
            &context,
            secrets
                .DEFAULT_TEST_DOMAIN
                .as_ref()
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

        let env_action = environment.clone();
        let selector = format!("appId={}", environment.applications[0].id);

        let result = environment.deploy_environment(&env_action, logger.clone(), &engine_config);
        assert!(matches!(result, TransactionResult::Ok));

        let ret = get_pods(
            context.clone(),
            Kind::Scw,
            environment.clone(),
            selector.as_str(),
            secrets.clone(),
        );
        assert_eq!(ret.is_ok(), true);
        assert_eq!(ret.unwrap().items.is_empty(), false);

        let result = environment.pause_environment(&env_action, logger.clone(), &engine_config_for_delete);
        assert!(matches!(result, TransactionResult::Ok));

        // Check that we have actually 0 pods running for this app
        let ret = get_pods(
            context.clone(),
            Kind::Scw,
            environment.clone(),
            selector.as_str(),
            secrets.clone(),
        );
        assert_eq!(ret.is_ok(), true);
        assert_eq!(ret.unwrap().items.is_empty(), true);

        // Check we can resume the env
        let ctx_resume = context.clone_not_same_execution_id();
        let engine_config_resume = scw_default_engine_config(&ctx_resume, logger.clone());
        let result = environment.deploy_environment(&env_action, logger.clone(), &engine_config_resume);
        assert!(matches!(result, TransactionResult::Ok));

        let ret = get_pods(
            context.clone(),
            Kind::Scw,
            environment.clone(),
            selector.as_str(),
            secrets.clone(),
        );
        assert_eq!(ret.is_ok(), true);
        assert_eq!(ret.unwrap().items.is_empty(), false);

        // Cleanup
        let result = environment.delete_environment(&env_action, logger, &engine_config_for_delete);
        assert!(matches!(result, TransactionResult::Ok));

        if let Err(e) = clean_environments(&context, vec![environment], secrets, SCW_TEST_ZONE) {
            warn!("cannot clean environments, error: {:?}", e);
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

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("SCALEWAY_TEST_ORGANIZATION_ID")
                .as_str(),
            secrets
                .SCALEWAY_TEST_CLUSTER_ID
                .as_ref()
                .expect("SCALEWAY_TEST_CLUSTER_ID")
                .as_str(),
        );
        let engine_config = scw_default_engine_config(&context, logger.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let engine_config_for_delete = scw_default_engine_config(&context_for_delete, logger.clone());
        let mut environment = test_utilities::common::working_minimal_environment(
            &context,
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
            .collect::<Vec<qovery_engine::models::Application>>();

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let env_action = environment.clone();
        let env_action_for_delete = environment_for_delete.clone();

        let result = environment.deploy_environment(&env_action, logger.clone(), &engine_config);
        assert!(matches!(result, TransactionResult::Ok));

        let result =
            environment_for_delete.delete_environment(&env_action_for_delete, logger, &engine_config_for_delete);
        assert!(matches!(result, TransactionResult::Ok));

        if let Err(e) = clean_environments(&context, vec![environment], secrets, SCW_TEST_ZONE) {
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
        init();

        let span = span!(Level::INFO, "test",);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("SCALEWAY_TEST_ORGANIZATION_ID")
                .as_str(),
            secrets
                .SCALEWAY_TEST_CLUSTER_ID
                .as_ref()
                .expect("SCALEWAY_TEST_CLUSTER_ID")
                .as_str(),
        );
        let engine_config = scw_default_engine_config(&context, logger.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let engine_config_for_delete = scw_default_engine_config(&context_for_delete, logger.clone());
        let environment = test_utilities::common::working_minimal_environment(
            &context,
            secrets
                .DEFAULT_TEST_DOMAIN
                .as_ref()
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;

        let env_action = environment.clone();
        let env_action_for_delete = environment_delete.clone();

        let result = environment.deploy_environment(&env_action, logger.clone(), &engine_config);
        assert!(matches!(result, TransactionResult::Ok));

        let result = environment_delete.delete_environment(&env_action_for_delete, logger, &engine_config_for_delete);
        assert!(matches!(result, TransactionResult::Ok));

        if let Err(e) = clean_environments(&context, vec![environment], secrets, SCW_TEST_ZONE) {
            warn!("cannot clean environments, error: {:?}", e);
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

        let secrets = FuncTestsSecrets::new();
        let logger = logger();
        let context = context(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("SCALEWAY_TEST_ORGANIZATION_ID")
                .as_str(),
            secrets
                .SCALEWAY_TEST_CLUSTER_ID
                .as_ref()
                .expect("SCALEWAY_TEST_CLUSTER_ID")
                .as_str(),
        );
        let engine_config = scw_default_engine_config(&context, logger.clone());
        let context_for_deletion = context.clone_not_same_execution_id();
        let engine_config_for_deletion = scw_default_engine_config(&context_for_deletion, logger.clone());

        let mut environment = test_utilities::common::working_minimal_environment(
            &context,
            secrets
                .DEFAULT_TEST_DOMAIN
                .as_ref()
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

        let storage_size: u16 = 10;
        environment.applications = environment
            .applications
            .into_iter()
            .map(|mut app| {
                app.storage = vec![Storage {
                    id: generate_id(),
                    name: "photos".to_string(),
                    storage_type: StorageType::Ssd,
                    size_in_gib: storage_size,
                    mount_point: "/mnt/photos".to_string(),
                    snapshot_retention_in_days: 0,
                }];
                app
            })
            .collect::<Vec<qovery_engine::models::Application>>();

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;

        let env_action = environment.clone();
        let env_action_delete = environment_delete.clone();

        let result = environment.deploy_environment(&env_action, logger.clone(), &engine_config);
        assert!(matches!(result, TransactionResult::Ok));

        match get_pvc(context.clone(), Kind::Scw, environment.clone(), secrets.clone()) {
            Ok(pvc) => assert_eq!(
                pvc.items.expect("No items in pvc")[0].spec.resources.requests.storage,
                format!("{}Gi", storage_size)
            ),
            Err(_) => assert!(false),
        };

        let result = environment_delete.delete_environment(&env_action_delete, logger, &engine_config_for_deletion);
        assert!(matches!(result, TransactionResult::Ok));

        if let Err(e) = clean_environments(&context, vec![environment], secrets, SCW_TEST_ZONE) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn deploy_a_working_environment_and_pause_it() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("SCALEWAY_TEST_ORGANIZATION_ID")
                .as_str(),
            secrets
                .SCALEWAY_TEST_CLUSTER_ID
                .as_ref()
                .expect("SCALEWAY_TEST_CLUSTER_ID")
                .as_str(),
        );
        let engine_config = scw_default_engine_config(&context, logger.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let engine_config_for_delete = scw_default_engine_config(&context_for_delete, logger.clone());
        let environment = test_utilities::common::working_minimal_environment(
            &context,
            secrets
                .DEFAULT_TEST_DOMAIN
                .as_ref()
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

        let ea = environment.clone();
        let selector = format!("appId={}", environment.applications[0].id);

        let result = environment.deploy_environment(&ea, logger.clone(), &engine_config);
        assert!(matches!(result, TransactionResult::Ok));

        let ret = get_pods(
            context.clone(),
            Kind::Scw,
            environment.clone(),
            selector.as_str(),
            secrets.clone(),
        );
        assert_eq!(ret.is_ok(), true);
        assert_eq!(ret.unwrap().items.is_empty(), false);

        let result = environment.pause_environment(&ea, logger.clone(), &engine_config_for_delete);
        assert!(matches!(result, TransactionResult::Ok));

        // Check that we have actually 0 pods running for this app
        let ret = get_pods(
            context.clone(),
            Kind::Scw,
            environment.clone(),
            selector.as_str(),
            secrets.clone(),
        );
        assert_eq!(ret.is_ok(), true);
        assert_eq!(ret.unwrap().items.is_empty(), true);

        // Check we can resume the env
        let ctx_resume = context.clone_not_same_execution_id();
        let engine_config_resume = scw_default_engine_config(&ctx_resume, logger.clone());
        let result = environment.deploy_environment(&ea, logger.clone(), &engine_config_resume);
        assert!(matches!(result, TransactionResult::Ok));

        let ret = get_pods(context, Kind::Scw, environment.clone(), selector.as_str(), secrets);
        assert_eq!(ret.is_ok(), true);
        assert_eq!(ret.unwrap().items.is_empty(), false);

        // Cleanup
        let result = environment.delete_environment(&ea, logger, &engine_config_for_delete);
        assert!(matches!(result, TransactionResult::Ok));
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

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("SCALEWAY_TEST_ORGANIZATION_ID")
                .as_str(),
            secrets
                .SCALEWAY_TEST_CLUSTER_ID
                .as_ref()
                .expect("SCALEWAY_TEST_CLUSTER_ID")
                .as_str(),
        );
        let engine_config = scw_default_engine_config(&context, logger.clone());
        let context_bis = context.clone_not_same_execution_id();
        let engine_config_bis = scw_default_engine_config(&context_bis, logger.clone());
        let context_for_deletion = context.clone_not_same_execution_id();
        let engine_config_for_deletion = scw_default_engine_config(&context_for_deletion, logger.clone());

        let mut environment = test_utilities::common::working_minimal_environment(
            &context,
            secrets
                .DEFAULT_TEST_DOMAIN
                .as_ref()
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

        let storage_size: u16 = 10;
        environment.applications = environment
            .applications
            .into_iter()
            .map(|mut app| {
                app.storage = vec![Storage {
                    id: generate_id(),
                    name: "photos".to_string(),
                    storage_type: StorageType::Ssd,
                    size_in_gib: storage_size,
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

        let env_action = environment.clone();
        let env_action_redeploy = environment_redeploy.clone();
        let env_action_delete = environment_delete.clone();

        let result = environment.deploy_environment(&env_action, logger.clone(), &engine_config);
        assert!(matches!(result, TransactionResult::Ok));

        match get_pvc(context.clone(), Kind::Scw, environment.clone(), secrets.clone()) {
            Ok(pvc) => assert_eq!(
                pvc.items.expect("No items in pvc")[0].spec.resources.requests.storage,
                format!("{}Gi", storage_size)
            ),
            Err(_) => assert!(false),
        };

        let app_name = format!("{}-0", &environment_check1.applications[0].name);
        let (_, number) = is_pod_restarted_env(
            context.clone(),
            Kind::Scw,
            environment_check1,
            app_name.as_str(),
            secrets.clone(),
        );

        let result = environment_redeploy.deploy_environment(&env_action_redeploy, logger.clone(), &engine_config_bis);
        assert!(matches!(result, TransactionResult::Ok));

        let (_, number2) = is_pod_restarted_env(
            context.clone(),
            Kind::Scw,
            environment_check2,
            app_name.as_str(),
            secrets.clone(),
        );

        // nothing changed in the app, so, it shouldn't be restarted
        assert!(number.eq(&number2));

        let result = environment_delete.delete_environment(&env_action_delete, logger, &engine_config_for_deletion);
        assert!(matches!(result, TransactionResult::Ok));

        if let Err(e) = clean_environments(&context, vec![environment], secrets, SCW_TEST_ZONE) {
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
        init();

        let span = span!(Level::INFO, "test", name = test_name,);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("SCALEWAY_TEST_ORGANIZATION_ID")
                .as_str(),
            secrets
                .SCALEWAY_TEST_CLUSTER_ID
                .as_ref()
                .expect("SCALEWAY_TEST_CLUSTER_ID")
                .as_str(),
        );
        let engine_config = scw_default_engine_config(&context, logger.clone());
        let context_for_not_working = context.clone_not_same_execution_id();
        let engine_config_for_not_working = scw_default_engine_config(&context_for_not_working, logger.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let engine_config_for_delete = scw_default_engine_config(&context_for_delete, logger.clone());

        // env part generation
        let environment = test_utilities::common::working_minimal_environment(
            &context,
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
                app.environment_vars = BTreeMap::default();
                app
            })
            .collect::<Vec<qovery_engine::models::Application>>();

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
        assert!(matches!(result, TransactionResult::UnrecoverableError(_, _)));

        let result = environment.deploy_environment(&env_action, logger.clone(), &engine_config);
        assert!(matches!(result, TransactionResult::Ok));

        let result = environment_for_delete.delete_environment(&env_action_delete, logger, &engine_config_for_delete);
        assert!(matches!(result, TransactionResult::Ok));

        if let Err(e) = clean_environments(&context, vec![environment], secrets, SCW_TEST_ZONE) {
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
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();

        // working env

        let secrets = FuncTestsSecrets::new();
        let context = context(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("SCALEWAY_TEST_ORGANIZATION_ID")
                .as_str(),
            secrets
                .SCALEWAY_TEST_CLUSTER_ID
                .as_ref()
                .expect("SCALEWAY_TEST_CLUSTER_ID")
                .as_str(),
        );
        let engine_config = scw_default_engine_config(&context, logger.clone());
        let environment = test_utilities::common::working_minimal_environment(
            &context,
            secrets
                .DEFAULT_TEST_DOMAIN
                .as_ref()
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

        // not working 1
        let context_for_not_working_1 = context.clone_not_same_execution_id();
        let engine_config_for_not_working_1 = scw_default_engine_config(&context_for_not_working_1, logger.clone());
        let mut not_working_env_1 = environment.clone();
        not_working_env_1.applications = not_working_env_1
            .applications
            .into_iter()
            .map(|mut app| {
                app.git_url = "https://gitlab.com/maathor/my-exit-container".to_string();
                app.branch = "master".to_string();
                app.commit_id = "55bc95a23fbf91a7699c28c5f61722d4f48201c9".to_string();
                app.environment_vars = BTreeMap::default();
                app
            })
            .collect::<Vec<qovery_engine::models::Application>>();

        // not working 2
        let context_for_not_working_2 = context.clone_not_same_execution_id();
        let engine_config_for_not_working_2 = scw_default_engine_config(&context_for_not_working_2, logger.clone());
        let not_working_env_2 = not_working_env_1.clone();

        // work for delete
        let context_for_delete = context.clone_not_same_execution_id();
        let engine_config_for_delete = scw_default_engine_config(&context_for_delete, logger.clone());
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
        assert!(matches!(
            result,
            TransactionResult::Rollback(_) | TransactionResult::UnrecoverableError(_, _)
        ));

        // FAIL and Rollback again
        let result = not_working_env_2.deploy_environment(
            &env_action_not_working_2,
            logger.clone(),
            &engine_config_for_not_working_2,
        );
        assert!(matches!(
            result,
            TransactionResult::Rollback(_) | TransactionResult::UnrecoverableError(_, _)
        ));

        // Should be working
        let result = environment.deploy_environment(&env_action, logger.clone(), &engine_config);
        assert!(matches!(result, TransactionResult::Ok));

        let result = delete_env.delete_environment(&env_action_delete, logger, &engine_config_for_delete);
        assert!(matches!(result, TransactionResult::Ok));

        if let Err(e) = clean_environments(&context, vec![environment], secrets, SCW_TEST_ZONE) {
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
        init();

        let span = span!(Level::INFO, "test", name = test_name,);
        let _enter = span.enter();

        let logger = logger();

        let secrets = FuncTestsSecrets::new();
        let context = context(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("SCALEWAY_TEST_ORGANIZATION_ID")
                .as_str(),
            secrets
                .SCALEWAY_TEST_CLUSTER_ID
                .as_ref()
                .expect("SCALEWAY_TEST_CLUSTER_ID")
                .as_str(),
        );
        let engine_config = scw_default_engine_config(&context, logger.clone());
        let environment = test_utilities::common::non_working_environment(
            &context,
            secrets
                .DEFAULT_TEST_DOMAIN
                .as_ref()
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

        let context_for_delete = context.clone_not_same_execution_id();
        let engine_config_for_delete = scw_default_engine_config(&context_for_delete, logger.clone());
        let mut delete_env = environment.clone();
        delete_env.action = Action::Delete;

        let env_action = environment.clone();
        let env_action_delete = delete_env.clone();

        let result = environment.deploy_environment(&env_action, logger.clone(), &engine_config);
        assert!(matches!(result, TransactionResult::UnrecoverableError(_, _)));

        let result = delete_env.delete_environment(&env_action_delete, logger, &engine_config_for_delete);
        assert!(matches!(result, TransactionResult::Ok));

        if let Err(e) = clean_environments(&context, vec![environment], secrets, SCW_TEST_ZONE) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn scaleway_kapsule_deploy_a_working_environment_with_sticky_session() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("SCALEWAY_TEST_ORGANIZATION_ID is not set in secrets")
                .as_str(),
            secrets
                .SCALEWAY_TEST_CLUSTER_ID
                .as_ref()
                .expect("SCALEWAY_TEST_CLUSTER_ID is not set in secrets")
                .as_str(),
        );
        let engine_config = scw_default_engine_config(&context, logger.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let engine_config_for_delete = scw_default_engine_config(&context_for_delete, logger.clone());
        let environment = test_utilities::common::environment_only_http_server_router_with_sticky_session(
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

        // let time for nginx to reload the config
        thread::sleep(Duration::from_secs(10));
        // checking cookie is properly set on the app
        assert!(routers_sessions_are_sticky(environment.routers.clone()));

        let result =
            environment_for_delete.delete_environment(&env_action_for_delete, logger, &engine_config_for_delete);
        assert!(matches!(result, TransactionResult::Ok));

        if let Err(e) = clean_environments(&context, vec![environment], secrets, SCW_TEST_ZONE) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

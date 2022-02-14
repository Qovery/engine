extern crate test_utilities;

use self::test_utilities::common::routers_sessions_are_sticky;
use self::test_utilities::scaleway::{clean_environments, SCW_TEST_ZONE};
use self::test_utilities::utilities::{
    context, engine_run_test, generate_id, get_pods, get_pvc, init, is_pod_restarted_env, logger, FuncTestsSecrets,
};
use ::function_name::named;
use qovery_engine::build_platform::{BuildPlatform, CacheResult};
use qovery_engine::cloud_provider::Kind;
use qovery_engine::container_registry::{ContainerRegistry, PullResult};
use qovery_engine::models::{Action, Clone2, EnvironmentAction, Port, Protocol, Storage, StorageType};
use qovery_engine::transaction::TransactionResult;
use std::collections::BTreeMap;
use std::time::SystemTime;
use test_utilities::common::Infrastructure;
use test_utilities::scaleway::container_registry_scw;
use test_utilities::utilities::build_platform_local_docker;
use tracing::{span, warn, Level};

// Note: All those tests relies on a test cluster running on Scaleway infrastructure.
// This cluster should be live in order to have those tests passing properly.

#[cfg(feature = "test-scw-self-hosted")]
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
        let context_for_delete = context.clone_not_same_execution_id();
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

        let env_action = EnvironmentAction::Environment(environment.clone());
        let env_action_for_delete = EnvironmentAction::Environment(environment_for_delete.clone());

        match environment.deploy_environment(Kind::Scw, &context, &env_action, logger.clone()) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        match environment_for_delete.delete_environment(Kind::Scw, &context_for_delete, &env_action_for_delete, logger)
        {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        if let Err(e) = clean_environments(&context, vec![environment.clone()], secrets.clone(), SCW_TEST_ZONE) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn test_build_cache() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

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

        let mut environment = test_utilities::common::working_minimal_environment(
            &context,
            secrets
                .DEFAULT_TEST_DOMAIN
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

        let scr = container_registry_scw(&context);
        let local_docker = build_platform_local_docker(&context);
        let app = environment.applications.first().unwrap();
        let image = app.to_image();

        let app_build = app.to_build();
        let _ = match local_docker.has_cache(&app_build) {
            Ok(CacheResult::Hit) => assert!(false),
            Ok(CacheResult::Miss(parent_build)) => assert!(true),
            Ok(CacheResult::MissWithoutParentBuild) => assert!(false),
            Err(err) => assert!(false),
        };

        let _ = match scr.pull(&image).unwrap() {
            PullResult::Some(_) => assert!(false),
            PullResult::None => assert!(true),
        };

        let build_result = local_docker.build(app.to_build(), false).unwrap();

        let _ = match scr.push(&build_result.build.image, false) {
            Ok(_) => assert!(true),
            Err(_) => assert!(false),
        };

        // TODO clean local docker cache

        let start_pull_time = SystemTime::now();
        let _ = match scr.pull(&build_result.build.image).unwrap() {
            PullResult::Some(_) => assert!(true),
            PullResult::None => assert!(false),
        };

        let pull_duration = SystemTime::now().duration_since(start_pull_time).unwrap();

        let _ = match local_docker.has_cache(&build_result.build) {
            Ok(CacheResult::Hit) => assert!(true),
            Ok(CacheResult::Miss(parent_build)) => assert!(false),
            Ok(CacheResult::MissWithoutParentBuild) => assert!(false),
            Err(err) => assert!(false),
        };

        let start_pull_time = SystemTime::now();
        let _ = match scr.pull(&image).unwrap() {
            PullResult::Some(_) => assert!(true),
            PullResult::None => assert!(false),
        };

        let pull_duration_2 = SystemTime::now().duration_since(start_pull_time).unwrap();

        if pull_duration_2.as_millis() > pull_duration.as_millis() {
            assert!(false);
        }

        return test_name.to_string();
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
        let context_for_delete = context.clone_not_same_execution_id();

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

        let env_action = EnvironmentAction::Environment(environment.clone());
        let env_action_for_delete = EnvironmentAction::Environment(environment_for_delete.clone());

        match environment.deploy_environment(Kind::Scw, &context, &env_action, logger.clone()) {
            TransactionResult::Ok => assert!(false),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(true),
        };

        match environment_for_delete.delete_environment(Kind::Scw, &context_for_delete, &env_action_for_delete, logger)
        {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(true),
        };

        if let Err(e) = clean_environments(&context, vec![environment.clone()], secrets.clone(), SCW_TEST_ZONE) {
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
        let context_for_delete = context.clone_not_same_execution_id();
        let environment = test_utilities::common::working_minimal_environment(
            &context,
            secrets
                .DEFAULT_TEST_DOMAIN
                .as_ref()
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

        let env_action = EnvironmentAction::Environment(environment.clone());
        let selector = format!("appId={}", environment.applications[0].id);

        match environment.deploy_environment(Kind::Scw, &context, &env_action, logger.clone()) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        let ret = get_pods(
            context.clone(),
            Kind::Scw,
            environment.clone(),
            selector.as_str(),
            secrets.clone(),
        );
        assert_eq!(ret.is_ok(), true);
        assert_eq!(ret.unwrap().items.is_empty(), false);

        match environment.pause_environment(Kind::Scw, &context_for_delete, &env_action, logger.clone()) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

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
        match environment.deploy_environment(Kind::Scw, &ctx_resume, &env_action, logger.clone()) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

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
        match environment.delete_environment(Kind::Scw, &context_for_delete, &env_action, logger) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        if let Err(e) = clean_environments(&context, vec![environment.clone()], secrets.clone(), SCW_TEST_ZONE) {
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
        let context_for_delete = context.clone_not_same_execution_id();
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

        let env_action = EnvironmentAction::Environment(environment.clone());
        let env_action_for_delete = EnvironmentAction::Environment(environment_for_delete.clone());

        match environment.deploy_environment(Kind::Scw, &context, &env_action, logger.clone()) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        match environment_for_delete.delete_environment(Kind::Scw, &context_for_delete, &env_action_for_delete, logger)
        {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        if let Err(e) = clean_environments(&context, vec![environment.clone()], secrets.clone(), SCW_TEST_ZONE) {
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
        let context_for_delete = context.clone_not_same_execution_id();
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

        let env_action = EnvironmentAction::Environment(environment.clone());
        let env_action_for_delete = EnvironmentAction::Environment(environment_delete.clone());

        match environment.deploy_environment(Kind::Scw, &context, &env_action, logger.clone()) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        match environment_delete.delete_environment(Kind::Scw, &context_for_delete, &env_action_for_delete, logger) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        if let Err(e) = clean_environments(&context, vec![environment.clone()], secrets.clone(), SCW_TEST_ZONE) {
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
        let context_for_deletion = context.clone_not_same_execution_id();

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

        let env_action = EnvironmentAction::Environment(environment.clone());
        let env_action_delete = EnvironmentAction::Environment(environment_delete.clone());

        match environment.deploy_environment(Kind::Scw, &context, &env_action, logger.clone()) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        match get_pvc(context.clone(), Kind::Scw, environment.clone(), secrets.clone()) {
            Ok(pvc) => assert_eq!(
                pvc.items.expect("No items in pvc")[0].spec.resources.requests.storage,
                format!("{}Gi", storage_size)
            ),
            Err(_) => assert!(false),
        };

        match environment_delete.delete_environment(Kind::Scw, &context_for_deletion, &env_action_delete, logger) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        if let Err(e) = clean_environments(&context, vec![environment.clone()], secrets.clone(), SCW_TEST_ZONE) {
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
        let context_for_delete = context.clone_not_same_execution_id();
        let environment = test_utilities::common::working_minimal_environment(
            &context,
            secrets
                .DEFAULT_TEST_DOMAIN
                .as_ref()
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

        let ea = EnvironmentAction::Environment(environment.clone());
        let selector = format!("appId={}", environment.applications[0].id);

        match environment.deploy_environment(Kind::Scw, &context, &ea, logger.clone()) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        let ret = get_pods(
            context.clone(),
            Kind::Scw,
            environment.clone(),
            selector.as_str(),
            secrets.clone(),
        );
        assert_eq!(ret.is_ok(), true);
        assert_eq!(ret.unwrap().items.is_empty(), false);

        match environment.pause_environment(Kind::Scw, &context_for_delete, &ea, logger.clone()) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

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
        match environment.deploy_environment(Kind::Scw, &ctx_resume, &ea, logger.clone()) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

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
        match environment.delete_environment(Kind::Scw, &context_for_delete, &ea, logger) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };
        return test_name.to_string();
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
        let context_bis = context.clone_not_same_execution_id();
        let context_for_deletion = context.clone_not_same_execution_id();

        let mut environment = test_utilities::common::working_minimal_environment(
            &context,
            secrets
                .clone()
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

        let env_action = EnvironmentAction::Environment(environment.clone());
        let env_action_redeploy = EnvironmentAction::Environment(environment_redeploy.clone());
        let env_action_delete = EnvironmentAction::Environment(environment_delete.clone());

        match environment.deploy_environment(Kind::Scw, &context, &env_action, logger.clone()) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

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
            app_name.clone().as_str(),
            secrets.clone(),
        );

        match environment_redeploy.deploy_environment(Kind::Scw, &context_bis, &env_action_redeploy, logger.clone()) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        let (_, number2) = is_pod_restarted_env(
            context.clone(),
            Kind::Scw,
            environment_check2,
            app_name.as_str(),
            secrets.clone(),
        );

        // nothing changed in the app, so, it shouldn't be restarted
        assert!(number.eq(&number2));

        match environment_delete.delete_environment(Kind::Scw, &context_for_deletion, &env_action_delete, logger) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        if let Err(e) = clean_environments(&context, vec![environment.clone()], secrets.clone(), SCW_TEST_ZONE) {
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
        let context_for_not_working = context.clone_not_same_execution_id();
        let context_for_delete = context.clone_not_same_execution_id();

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
        let env_action = EnvironmentAction::Environment(environment.clone());
        let env_action_not_working = EnvironmentAction::Environment(environment_for_not_working.clone());
        let env_action_delete = EnvironmentAction::Environment(environment_for_delete.clone());

        match environment_for_not_working.deploy_environment(
            Kind::Scw,
            &context_for_not_working,
            &env_action_not_working,
            logger.clone(),
        ) {
            TransactionResult::Ok => assert!(false),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(true),
        };
        match environment.deploy_environment(Kind::Scw, &context, &env_action, logger.clone()) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };
        match environment_for_delete.delete_environment(Kind::Scw, &context_for_delete, &env_action_delete, logger) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        if let Err(e) = clean_environments(&context, vec![environment.clone()], secrets.clone(), SCW_TEST_ZONE) {
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
        let not_working_env_2 = not_working_env_1.clone();

        // work for delete
        let context_for_delete = context.clone_not_same_execution_id();
        let mut delete_env = environment.clone();
        delete_env.action = Action::Delete;

        let env_action = EnvironmentAction::Environment(environment.clone());
        let env_action_not_working_1 = EnvironmentAction::Environment(not_working_env_1.clone());
        let env_action_not_working_2 = EnvironmentAction::Environment(not_working_env_2.clone());
        let env_action_delete = EnvironmentAction::Environment(delete_env.clone());

        // OK
        match environment.deploy_environment(Kind::Scw, &context, &env_action, logger.clone()) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        // FAIL and rollback
        match not_working_env_1.deploy_environment(
            Kind::Scw,
            &context_for_not_working_1,
            &env_action_not_working_1,
            logger.clone(),
        ) {
            TransactionResult::Ok => assert!(false),
            TransactionResult::Rollback(_) => assert!(true),
            TransactionResult::UnrecoverableError(_, _) => assert!(true),
        };

        // FAIL and Rollback again
        match not_working_env_2.deploy_environment(
            Kind::Scw,
            &context_for_not_working_2,
            &env_action_not_working_2,
            logger.clone(),
        ) {
            TransactionResult::Ok => assert!(false),
            TransactionResult::Rollback(_) => assert!(true),
            TransactionResult::UnrecoverableError(_, _) => assert!(true),
        };

        // Should be working
        match environment.deploy_environment(Kind::Scw, &context, &env_action, logger.clone()) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        match delete_env.delete_environment(Kind::Scw, &context_for_delete, &env_action_delete, logger) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        if let Err(e) = clean_environments(&context, vec![environment.clone()], secrets.clone(), SCW_TEST_ZONE) {
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
        let environment = test_utilities::common::non_working_environment(
            &context,
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
        let env_action_delete = EnvironmentAction::Environment(delete_env.clone());

        match environment.deploy_environment(Kind::Scw, &context, &env_action, logger.clone()) {
            TransactionResult::Ok => assert!(false),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(true),
        };

        match delete_env.delete_environment(Kind::Scw, &context_for_delete, &env_action_delete, logger) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        if let Err(e) = clean_environments(&context, vec![environment.clone()], secrets.clone(), SCW_TEST_ZONE) {
            warn!("cannot clean environments, error: {:?}", e);
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

        let logger = logger();

        // context for non working environment

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
        let test_domain = secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets");

        let environment = test_utilities::common::non_working_environment(&context, test_domain.as_str());
        let failover_environment = test_utilities::common::working_minimal_environment(&context, test_domain.as_str());

        // context for deletion
        let context_deletion = context.clone_not_same_execution_id();
        let mut delete_env =
            test_utilities::common::working_minimal_environment(&context_deletion, test_domain.as_str());
        delete_env.action = Action::Delete;

        let env_action_delete = EnvironmentAction::Environment(delete_env.clone());
        let env_action = EnvironmentAction::EnvironmentWithFailover(environment.clone(), failover_environment.clone());

        match environment.deploy_environment(Kind::Scw, &context, &env_action, logger.clone()) {
            TransactionResult::Ok => assert!(false),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(true),
        };

        match delete_env.delete_environment(Kind::Scw, &context_deletion, &env_action_delete, logger) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        if let Err(e) = clean_environments(
            &context,
            vec![environment.clone(), failover_environment.clone()],
            secrets.clone(),
            SCW_TEST_ZONE,
        ) {
            warn!("cannot clean environments, error: {:?}", e);
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
        let test_domain = secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets");

        let environment = test_utilities::common::non_working_environment(&context, test_domain.as_str());
        let failover_environment = test_utilities::common::non_working_environment(&context, test_domain.as_str());

        let context_for_deletion = context.clone_not_same_execution_id();
        let mut delete_env =
            test_utilities::common::non_working_environment(&context_for_deletion, test_domain.as_str());
        delete_env.action = Action::Delete;

        // environment action initialize
        let env_action_delete = EnvironmentAction::Environment(delete_env.clone());
        let env_action = EnvironmentAction::EnvironmentWithFailover(environment.clone(), failover_environment.clone());

        match environment.deploy_environment(Kind::Scw, &context, &env_action, logger.clone()) {
            TransactionResult::Ok => assert!(false),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(true),
        };

        match delete_env.delete_environment(Kind::Scw, &context_for_deletion, &env_action_delete, logger) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(true),
        };

        if let Err(e) = clean_environments(
            &context,
            vec![environment.clone(), failover_environment.clone()],
            secrets.clone(),
            SCW_TEST_ZONE,
        ) {
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
        let context_for_delete = context.clone_not_same_execution_id();
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

        let env_action = EnvironmentAction::Environment(environment.clone());
        let env_action_for_delete = EnvironmentAction::Environment(environment_for_delete.clone());

        match environment.deploy_environment(Kind::Scw, &context, &env_action, logger.clone()) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        // checking cookie is properly set on the app
        assert!(routers_sessions_are_sticky(environment.routers.clone()));

        match environment_for_delete.delete_environment(Kind::Scw, &context_for_delete, &env_action_for_delete, logger)
        {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        if let Err(e) = clean_environments(&context, vec![environment.clone()], secrets.clone(), SCW_TEST_ZONE) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

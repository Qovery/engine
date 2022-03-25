extern crate test_utilities;

use self::test_utilities::common::{routers_sessions_are_sticky, Infrastructure};
use self::test_utilities::utilities::{
    engine_run_test, generate_id, get_pods, get_pvc, is_pod_restarted_env, logger, FuncTestsSecrets,
};
use ::function_name::named;
use qovery_engine::cloud_provider::Kind;
use qovery_engine::cmd::kubectl::kubernetes_get_all_pdbs;
use qovery_engine::models::{Action, CloneForTest, Port, Protocol, Storage, StorageType};
use qovery_engine::transaction::TransactionResult;
use std::collections::BTreeMap;
use std::thread;
use std::time::Duration;
use test_utilities::aws::aws_default_engine_config;
use test_utilities::utilities::{context, init, kubernetes_config_path};
use tracing::{span, Level};

// TODO:
//   - Tests that applications are always restarted when receiving a CREATE action
//     see: https://github.com/Qovery/engine/pull/269

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
        let secrets = FuncTestsSecrets::new();
        let context = context(
            secrets
                .AWS_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("AWS_TEST_ORGANIZATION_ID is not set")
                .as_str(),
            secrets
                .AWS_TEST_CLUSTER_ID
                .as_ref()
                .expect("AWS_TEST_CLUSTER_ID is not set")
                .as_str(),
        );
        let engine_config = aws_default_engine_config(&context, logger.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let engine_config_for_delete = aws_default_engine_config(&context_for_delete, logger.clone());

        let mut environment = test_utilities::common::working_minimal_environment(
            &context,
            secrets
                .DEFAULT_TEST_DOMAIN
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

        let mut environment_for_delete = environment.clone();
        environment.routers = vec![];
        environment_for_delete.routers = vec![];
        environment_for_delete.action = Action::Delete;

        let ea = environment.clone();
        let ea_delete = environment_for_delete.clone();

        let ret = environment.deploy_environment(&ea, logger.clone(), &engine_config);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = environment_for_delete.delete_environment(&ea_delete, logger, &engine_config_for_delete);
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
        let context = context(
            secrets
                .AWS_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("AWS_TEST_ORGANIZATION_ID is not set")
                .as_str(),
            secrets
                .AWS_TEST_CLUSTER_ID
                .as_ref()
                .expect("AWS_TEST_CLUSTER_ID is not set")
                .as_str(),
        );
        let engine_config = aws_default_engine_config(&context, logger.clone());

        let context_for_delete = context.clone_not_same_execution_id();
        let engine_config_for_delete = aws_default_engine_config(&context_for_delete, logger.clone());
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

        let ret = environment.deploy_environment(&ea, logger.clone(), &engine_config);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = get_pods(
            context.clone(),
            Kind::Aws,
            environment.clone(),
            selector.as_str(),
            secrets.clone(),
        );
        assert_eq!(ret.is_ok(), true);
        assert_eq!(ret.unwrap().items.is_empty(), false);

        let ret = environment.pause_environment(&ea, logger.clone(), &engine_config_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        // Check that we have actually 0 pods running for this app
        let ret = get_pods(
            context.clone(),
            Kind::Aws,
            environment.clone(),
            selector.as_str(),
            secrets.clone(),
        );
        assert_eq!(ret.is_ok(), true);
        assert_eq!(ret.unwrap().items.is_empty(), true);

        let kubernetes_config = kubernetes_config_path(context.clone(), Kind::Aws, "/tmp", secrets.clone());
        let mut pdbs = kubernetes_get_all_pdbs(
            kubernetes_config.as_ref().expect("Unable to get kubeconfig").clone(),
            vec![
                (
                    "AWS_ACCESS_KEY_ID",
                    secrets
                        .AWS_ACCESS_KEY_ID
                        .as_ref()
                        .expect("AWS_ACCESS_KEY_ID is not set")
                        .as_str(),
                ),
                (
                    "AWS_SECRET_ACCESS_KEY",
                    secrets
                        .AWS_SECRET_ACCESS_KEY
                        .as_ref()
                        .expect("AWS_SECRET_ACCESS_KEY is not set")
                        .as_str(),
                ),
                (
                    "AWS_DEFAULT_REGION",
                    secrets
                        .AWS_DEFAULT_REGION
                        .as_ref()
                        .expect("AWS_DEFAULT_REGION is not set")
                        .as_str(),
                ),
            ],
            None,
        );
        for pdb in pdbs.expect("Unable to get pdbs").items.expect("Unable to get pdbs") {
            assert_eq!(pdb.metadata.name.contains(&environment.applications[0].name), false)
        }

        // Check we can resume the env
        let ctx_resume = context.clone_not_same_execution_id();
        let engine_config_resume = aws_default_engine_config(&ctx_resume, logger.clone());
        let ret = environment.deploy_environment(&ea, logger.clone(), &engine_config_resume);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = get_pods(context, Kind::Aws, environment.clone(), selector.as_str(), secrets.clone());
        assert_eq!(ret.is_ok(), true);
        assert_eq!(ret.unwrap().items.is_empty(), false);

        pdbs = kubernetes_get_all_pdbs(
            kubernetes_config.as_ref().expect("Unable to get kubeconfig").clone(),
            vec![
                (
                    "AWS_ACCESS_KEY_ID",
                    secrets
                        .AWS_ACCESS_KEY_ID
                        .as_ref()
                        .expect("AWS_ACCESS_KEY_ID is not set")
                        .as_str(),
                ),
                (
                    "AWS_SECRET_ACCESS_KEY",
                    secrets
                        .AWS_SECRET_ACCESS_KEY
                        .as_ref()
                        .expect("AWS_SECRET_ACCESS_KEY is not set")
                        .as_str(),
                ),
                (
                    "AWS_DEFAULT_REGION",
                    secrets
                        .AWS_DEFAULT_REGION
                        .as_ref()
                        .expect("AWS_DEFAULT_REGION is not set")
                        .as_str(),
                ),
            ],
            None,
        );
        let mut filtered_pdb = false;
        for pdb in pdbs.expect("Unable to get pdbs").items.expect("Unable to get pdbs") {
            if pdb.metadata.name.contains(&environment.applications[0].name) {
                filtered_pdb = true;
                break;
            }
        }
        assert!(filtered_pdb);

        // Cleanup
        let ret = environment.delete_environment(&ea, logger, &engine_config_for_delete);
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
        let context = context(
            secrets
                .AWS_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("AWS_TEST_ORGANIZATION_ID is not set")
                .as_str(),
            secrets
                .AWS_TEST_CLUSTER_ID
                .as_ref()
                .expect("AWS_TEST_CLUSTER_ID is not set")
                .as_str(),
        );
        let engine_config = aws_default_engine_config(&context, logger.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let engine_config_for_delete = aws_default_engine_config(&context_for_delete, logger.clone());

        let mut environment = test_utilities::common::non_working_environment(
            &context,
            secrets
                .DEFAULT_TEST_DOMAIN
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );
        environment.routers = vec![];

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;

        let ea = environment.clone();
        let ea_delete = environment_delete.clone();

        let ret = environment.deploy_environment(&ea, logger.clone(), &engine_config);
        assert!(matches!(ret, TransactionResult::UnrecoverableError(_, _)));

        let ret = environment_delete.delete_environment(&ea_delete, logger, &engine_config_for_delete);
        assert!(matches!(
            ret,
            TransactionResult::Ok | TransactionResult::UnrecoverableError(_, _)
        ));

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
        let context = context(
            secrets
                .AWS_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("AWS_TEST_ORGANIZATION_ID is not set")
                .as_str(),
            secrets
                .AWS_TEST_CLUSTER_ID
                .as_ref()
                .expect("AWS_TEST_CLUSTER_ID is not set")
                .as_str(),
        );
        let engine_config = aws_default_engine_config(&context, logger.clone());
        let context_for_deletion = context.clone_not_same_execution_id();
        let engine_config_for_deletion = aws_default_engine_config(&context_for_deletion, logger.clone());
        let mut environment = test_utilities::common::working_minimal_environment(
            &context,
            secrets
                .DEFAULT_TEST_DOMAIN
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

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;

        let ea = environment.clone();
        let ea_delete = environment_delete.clone();

        let ret = environment.deploy_environment(&ea, logger.clone(), &engine_config);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = environment_delete.delete_environment(&ea_delete, logger, &engine_config_for_deletion);
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
        let context = context(
            secrets
                .AWS_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("AWS_TEST_ORGANIZATION_ID is not set")
                .as_str(),
            secrets
                .AWS_TEST_CLUSTER_ID
                .as_ref()
                .expect("AWS_TEST_CLUSTER_ID is not set")
                .as_str(),
        );
        let engine_config = aws_default_engine_config(&context, logger.clone());
        let context_for_deletion = context.clone_not_same_execution_id();
        let engine_config_for_deletion = aws_default_engine_config(&context_for_deletion, logger.clone());
        let mut environment = test_utilities::common::working_minimal_environment(
            &context,
            secrets
                .DEFAULT_TEST_DOMAIN
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

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;

        let ea = environment.clone();
        let ea_delete = environment_delete.clone();

        let ret = environment.deploy_environment(&ea, logger.clone(), &engine_config);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = environment_delete.delete_environment(&ea_delete, logger, &engine_config_for_deletion);
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
        let context = context(
            secrets
                .AWS_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("AWS_TEST_ORGANIZATION_ID is not set")
                .as_str(),
            secrets
                .AWS_TEST_CLUSTER_ID
                .as_ref()
                .expect("AWS_TEST_CLUSTER_ID is not set")
                .as_str(),
        );
        let engine_config = aws_default_engine_config(&context, logger.clone());
        let context_for_deletion = context.clone_not_same_execution_id();
        let engine_config_for_deletion = aws_default_engine_config(&context_for_deletion, logger.clone());
        let environment = test_utilities::common::working_minimal_environment(
            &context,
            secrets
                .DEFAULT_TEST_DOMAIN
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;

        let ea = environment.clone();
        let ea_delete = environment_delete.clone();

        let ret = environment.deploy_environment(&ea, logger.clone(), &engine_config);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = environment_delete.delete_environment(&ea_delete, logger, &engine_config_for_deletion);
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
        let context = context(
            secrets
                .AWS_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("AWS_TEST_ORGANIZATION_ID is not set")
                .as_str(),
            secrets
                .AWS_TEST_CLUSTER_ID
                .as_ref()
                .expect("AWS_TEST_CLUSTER_ID is not set")
                .as_str(),
        );
        let engine_config = aws_default_engine_config(&context, logger.clone());
        let context_for_deletion = context.clone_not_same_execution_id();
        let engine_config_for_deletion = aws_default_engine_config(&context_for_deletion, logger.clone());

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

        let ea = environment.clone();
        let ea_delete = environment_delete.clone();

        let ret = environment.deploy_environment(&ea, logger.clone(), &engine_config);
        assert!(matches!(ret, TransactionResult::Ok));

        match get_pvc(context, Kind::Aws, environment, secrets) {
            Ok(pvc) => assert_eq!(
                pvc.items.expect("No items in pvc")[0].spec.resources.requests.storage,
                format!("{}Gi", storage_size)
            ),
            Err(_) => assert!(false),
        };

        let ret = environment_delete.delete_environment(&ea_delete, logger, &engine_config_for_deletion);
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
        let context = context(
            secrets
                .AWS_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("AWS_TEST_ORGANIZATION_ID is not set")
                .as_str(),
            secrets
                .AWS_TEST_CLUSTER_ID
                .as_ref()
                .expect("AWS_TEST_CLUSTER_ID is not set")
                .as_str(),
        );
        let engine_config = aws_default_engine_config(&context, logger.clone());
        let context_bis = context.clone_not_same_execution_id();
        let engine_config_bis = aws_default_engine_config(&context_bis, logger.clone());
        let context_for_deletion = context.clone_not_same_execution_id();
        let engine_config_for_deletion = aws_default_engine_config(&context_for_deletion, logger.clone());

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

        let ea = environment.clone();
        let ea2 = environment_redeploy.clone();
        let ea_delete = environment_delete.clone();

        let ret = environment.deploy_environment(&ea, logger.clone(), &engine_config);
        assert!(matches!(ret, TransactionResult::Ok));

        match get_pvc(context.clone(), Kind::Aws, environment, secrets.clone()) {
            Ok(pvc) => assert_eq!(
                pvc.items.expect("No items in pvc")[0].spec.resources.requests.storage,
                format!("{}Gi", storage_size)
            ),
            Err(_) => assert!(false),
        };

        let app_name = format!("{}-0", &environment_check1.applications[0].name);
        let (_, number) = is_pod_restarted_env(
            context.clone(),
            Kind::Aws,
            environment_check1,
            app_name.as_str(),
            secrets.clone(),
        );

        let ret = environment_redeploy.deploy_environment(&ea2, logger.clone(), &engine_config_bis);
        assert!(matches!(ret, TransactionResult::Ok));

        let (_, number2) = is_pod_restarted_env(context, Kind::Aws, environment_check2, app_name.as_str(), secrets);
        //nothing change in the app, so, it shouldn't be restarted
        assert!(number.eq(&number2));
        let ret = environment_delete.delete_environment(&ea_delete, logger, &engine_config_for_deletion);
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
        let context = context(
            secrets
                .AWS_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("AWS_TEST_ORGANIZATION_ID is not set")
                .as_str(),
            secrets
                .AWS_TEST_CLUSTER_ID
                .as_ref()
                .expect("AWS_TEST_CLUSTER_ID is not set")
                .as_str(),
        );
        let engine_config = aws_default_engine_config(&context, logger.clone());
        let context_for_not_working = context.clone_not_same_execution_id();
        let engine_config_for_not_working = aws_default_engine_config(&context_for_not_working, logger.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let engine_config_for_delete = aws_default_engine_config(&context_for_delete, logger.clone());

        // env part generation
        let environment = test_utilities::common::working_minimal_environment(
            &context,
            secrets
                .DEFAULT_TEST_DOMAIN
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
        let ea = environment.clone();
        let ea_not_working = environment_for_not_working.clone();
        let ea_delete = environment_for_delete.clone();

        let ret = environment_for_not_working.deploy_environment(
            &ea_not_working,
            logger.clone(),
            &engine_config_for_not_working,
        );
        assert!(matches!(ret, TransactionResult::UnrecoverableError(_, _)));

        let ret = environment.deploy_environment(&ea, logger.clone(), &engine_config);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = environment_for_delete.delete_environment(&ea_delete, logger, &engine_config_for_delete);
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
        let context = context(
            secrets
                .AWS_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("AWS_TEST_ORGANIZATION_ID is not set")
                .as_str(),
            secrets
                .AWS_TEST_CLUSTER_ID
                .as_ref()
                .expect("AWS_TEST_CLUSTER_ID is not set")
                .as_str(),
        );
        let engine_config = aws_default_engine_config(&context, logger.clone());
        let environment = test_utilities::common::working_minimal_environment(
            &context,
            secrets
                .DEFAULT_TEST_DOMAIN
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

        // not working 1
        let context_for_not_working_1 = context.clone_not_same_execution_id();
        let engine_config_for_not_working_1 = aws_default_engine_config(&context_for_not_working_1, logger.clone());
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
        let engine_config_for_not_working_2 = aws_default_engine_config(&context_for_not_working_2, logger.clone());
        let not_working_env_2 = not_working_env_1.clone();

        // work for delete
        let context_for_delete = context.clone_not_same_execution_id();
        let engine_config_for_delete = aws_default_engine_config(&context_for_delete, logger.clone());
        let mut delete_env = environment.clone();
        delete_env.action = Action::Delete;

        let ea = environment.clone();
        let ea_not_working_1 = not_working_env_1.clone();
        let ea_not_working_2 = not_working_env_2.clone();
        let ea_delete = delete_env.clone();

        // OK
        let ret = environment.deploy_environment(&ea, logger.clone(), &engine_config);
        assert!(matches!(ret, TransactionResult::Ok));

        // FAIL and rollback
        let ret =
            not_working_env_1.deploy_environment(&ea_not_working_1, logger.clone(), &engine_config_for_not_working_1);
        assert!(matches!(
            ret,
            TransactionResult::Rollback(_) | TransactionResult::UnrecoverableError(_, _)
        ));

        // FAIL and Rollback again
        let ret =
            not_working_env_2.deploy_environment(&ea_not_working_2, logger.clone(), &engine_config_for_not_working_2);
        assert!(matches!(
            ret,
            TransactionResult::Rollback(_) | TransactionResult::UnrecoverableError(_, _)
        ));

        // Should be working
        let ret = environment.deploy_environment(&ea, logger.clone(), &engine_config);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = delete_env.delete_environment(&ea_delete, logger, &engine_config_for_delete);
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
        let context = context(
            secrets
                .AWS_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("AWS_TEST_ORGANIZATION_ID is not set")
                .as_str(),
            secrets
                .AWS_TEST_CLUSTER_ID
                .as_ref()
                .expect("AWS_TEST_CLUSTER_ID is not set")
                .as_str(),
        );
        let engine_config = aws_default_engine_config(&context, logger.clone());
        let environment = test_utilities::common::non_working_environment(
            &context,
            secrets
                .DEFAULT_TEST_DOMAIN
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

        let context_for_delete = context.clone_not_same_execution_id();
        let engine_config_for_delete = aws_default_engine_config(&context_for_delete, logger.clone());
        let mut delete_env = environment.clone();
        delete_env.action = Action::Delete;

        let ea = environment.clone();
        let ea_delete = delete_env.clone();

        let ret = environment.deploy_environment(&ea, logger.clone(), &engine_config);
        assert!(matches!(ret, TransactionResult::UnrecoverableError(_, _)));

        let ret = delete_env.delete_environment(&ea_delete, logger, &engine_config_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        test_name.to_string()
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn aws_eks_deploy_a_working_environment_with_sticky_session() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context(
            secrets
                .AWS_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("AWS_TEST_ORGANIZATION_ID in secrets")
                .as_str(),
            secrets
                .AWS_TEST_CLUSTER_ID
                .as_ref()
                .expect("AWS_TEST_CLUSTER_ID is not set in secrets")
                .as_str(),
        );
        let engine_config = aws_default_engine_config(&context, logger.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let engine_config_for_delete = aws_default_engine_config(&context_for_delete, logger.clone());
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

        let ret = environment.deploy_environment(&env_action, logger.clone(), &engine_config);
        assert!(matches!(ret, TransactionResult::Ok));

        // let time for nginx to reload the config
        thread::sleep(Duration::from_secs(10));
        // checking if cookie is properly set on the app
        assert!(routers_sessions_are_sticky(environment.routers));

        let ret = environment_for_delete.delete_environment(&env_action_for_delete, logger, &engine_config_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        test_name.to_string()
    })
}

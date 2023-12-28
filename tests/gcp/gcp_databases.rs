/*
use function_name::named;
use qovery_engine::cloud_provider::Kind;
use qovery_engine::io_models::context::CloneForTest;
use qovery_engine::io_models::Action;
use qovery_engine::transaction::TransactionResult;
use tracing::{span, Level};

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
#[ignore]
fn deploy_an_environment_with_3_databases_and_3_apps() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let secrets = FuncTestsSecrets::new();
        let logger = logger();
        let metrics_registry = metrics_registry();
        let context = context_for_resource(
            secrets
                .GCP_TEST_ORGANIZATION_LONG_ID
                .expect("GCP_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .GCP_TEST_CLUSTER_LONG_ID
                .expect("GCP_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = gcp_default_infra_config(&context, logger.clone(), metrics_registry.clone());
        let context_for_deletion = context.clone_not_same_execution_id();
        let infra_ctx_for_deletion =
            gcp_default_infra_config(&context_for_deletion, logger.clone(), metrics_registry.clone());
        let environment = helpers::database::environment_3_apps_3_databases(
            &context,
            None,
            &GCP_DATABASE_DISK_TYPE.to_k8s_storage_class(),
            Kind::Gcp,
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

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
#[ignore]
fn deploy_an_environment_with_db_and_pause_it() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let secrets = FuncTestsSecrets::new();
        let logger = logger();
        let metrics_registry = metrics_registry();
        let cluster_id = secrets
            .GCP_TEST_CLUSTER_LONG_ID
            .expect("GCP_TEST_CLUSTER_LONG_ID is not set");
        let context = context_for_resource(
            secrets
                .GCP_TEST_ORGANIZATION_LONG_ID
                .expect("GCP_TEST_ORGANIZATION_LONG_ID is not set"),
            cluster_id,
        );
        let infra_ctx = gcp_default_infra_config(&context, logger.clone(), metrics_registry.clone());
        let context_for_deletion = context.clone_not_same_execution_id();
        let infra_ctx_for_deletion =
            gcp_default_infra_config(&context_for_deletion, logger.clone(), metrics_registry.clone());
        let environment = helpers::environment::environment_2_app_2_routers_1_psql(
            &context,
            secrets
                .clone()
                .DEFAULT_TEST_DOMAIN
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
            None,
            &GCP_DATABASE_DISK_TYPE.to_k8s_storage_class(),
            Kind::Gcp,
        );

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;
        let ea = environment.clone();
        let ea_delete = environment_delete.clone();

        let ret = environment.deploy_environment(&ea, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = environment.pause_environment(&ea, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        // Check that we have actually 0 pods running for this db
        let ret = get_pods(&infra_ctx, Kind::Gcp, &environment, &environment.databases[0].long_id, secrets);
        assert!(ret.is_ok());
        assert!(ret.unwrap().items.is_empty());

        let ret = environment_delete.delete_environment(&ea_delete, &infra_ctx_for_deletion);
        assert!(matches!(ret, TransactionResult::Ok));

        test_name.to_string()
    })
}
 */

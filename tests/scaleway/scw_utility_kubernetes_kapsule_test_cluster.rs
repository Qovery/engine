use crate::helpers::scaleway::scw_default_infra_config;
use crate::helpers::utilities::{
    context_for_cluster, engine_run_test, init, logger, metrics_registry, FuncTestsSecrets,
};
use ::function_name::named;
use tracing::{span, Level};

use qovery_engine::transaction::{Transaction, TransactionResult};

// Warning: This test shouldn't be ran by CI
// Note: this test creates the test cluster where all application tests will be ran
// This is not really a test but a convenient way to create the test cluster if needed to be manually created at some point.
#[allow(dead_code)]
#[named]
#[test]
#[ignore]
fn create_scaleway_kubernetes_kapsule_test_cluster() {
    let secrets = FuncTestsSecrets::new();
    let test_name = function_name!();

    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "utility", name = test_name);
        let _enter = span.enter();

        let organization_id = secrets
            .SCALEWAY_TEST_ORGANIZATION_LONG_ID
            .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID");
        let cluster_id = secrets
            .SCALEWAY_TEST_CLUSTER_LONG_ID
            .expect("SCALEWAY_TEST_CLUSTER_LONG_ID");

        let logger = logger();
        let metrics_registry = metrics_registry();
        let context = context_for_cluster(organization_id, cluster_id, None);
        let engine = scw_default_infra_config(&context, logger.clone(), metrics_registry.clone());
        let mut tx = Transaction::new(&engine).unwrap();

        // Deploy
        if let Err(err) = tx.create_kubernetes() {
            panic!("{err:?}")
        }

        assert!(matches!(tx.commit(), TransactionResult::Ok));

        test_name.to_string()
    });
}

// Warning: This test shouldn't be ran by CI
// Note: this test destroys the test cluster where all application tests will be ran
// This is not really a test but a convenient way to create the test cluster if needed to be manually destroyed at some point.
#[allow(dead_code)]
#[named]
#[test]
#[ignore]
fn destroy_scaleway_kubernetes_kapsule_test_cluster() {
    let secrets = FuncTestsSecrets::new();
    let test_name = function_name!();

    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "utility", name = test_name);
        let _enter = span.enter();

        let organization_id = secrets
            .SCALEWAY_TEST_ORGANIZATION_LONG_ID
            .expect("SCALEWAY_TEST_ORGANIZATION_LONG_ID");
        let cluster_id = secrets
            .SCALEWAY_TEST_CLUSTER_LONG_ID
            .expect("SCALEWAY_TEST_CLUSTER_LONG_ID");

        let logger = logger();
        let metrics_registry = metrics_registry();
        let context = context_for_cluster(organization_id, cluster_id, None);
        let engine = scw_default_infra_config(&context, logger.clone(), metrics_registry.clone());
        let mut tx = Transaction::new(&engine).unwrap();

        // Destroy
        if let Err(err) = tx.delete_kubernetes() {
            panic!("{err:?}")
        }
        let ret = tx.commit();
        assert!(matches!(ret, TransactionResult::Ok));

        test_name.to_string()
    });
}

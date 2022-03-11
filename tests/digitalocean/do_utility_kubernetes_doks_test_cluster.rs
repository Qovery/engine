extern crate test_utilities;

use self::test_utilities::utilities::{context, engine_run_test, init, logger, FuncTestsSecrets};
use ::function_name::named;
use qovery_engine::cloud_provider::digitalocean::DO;
use test_utilities::digitalocean::{do_default_engine_config, DO_KUBERNETES_VERSION, DO_TEST_REGION};
use tracing::{span, Level};

use self::test_utilities::common::Cluster;
use qovery_engine::transaction::{Transaction, TransactionResult};

// Warning: This test shouldn't be ran by CI
// Note: this test creates the test cluster where all application tests will be ran
// This is not really a test but a convenient way to create the test cluster if needed to be manually created at some point.
#[allow(dead_code)]
#[named]
#[test]
#[ignore]
fn create_digitalocean_kubernetes_doks_test_cluster() {
    let secrets = FuncTestsSecrets::new();
    let test_name = function_name!();

    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "utility", name = test_name);
        let _enter = span.enter();

        let organization_id = secrets
            .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set");
        let cluster_id = secrets
            .DIGITAL_OCEAN_TEST_CLUSTER_ID
            .as_ref()
            .expect("DIGITAL_OCEAN_TEST_CLUSTER_ID is not set");

        let logger = logger();
        let context = context(organization_id.as_str(), cluster_id.as_str());
        let engine = do_default_engine_config(&context, logger.clone());
        let mut tx = Transaction::new(&engine, logger.clone(), Box::new(|| false), Box::new(|_| {})).unwrap();

        // Deploy
        if let Err(err) = tx.create_kubernetes() {
            panic!("{:?}", err)
        }
        let ret = tx.commit();
        assert!(matches!(ret, TransactionResult::Ok));

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
fn destroy_digitalocean_kubernetes_doks_test_cluster() {
    let secrets = FuncTestsSecrets::new();
    let test_name = function_name!();

    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "utility", name = test_name);
        let _enter = span.enter();

        let organization_id = secrets
            .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set");
        let cluster_id = secrets
            .DIGITAL_OCEAN_TEST_CLUSTER_ID
            .as_ref()
            .expect("DIGITAL_OCEAN_TEST_CLUSTER_ID is not set");

        let logger = logger();
        let context = context(organization_id.as_str(), cluster_id.as_str());
        let engine = do_default_engine_config(&context, logger.clone());
        let mut tx = Transaction::new(&engine, logger.clone(), Box::new(|| false), Box::new(|_| {})).unwrap();

        // Destroy
        if let Err(err) = tx.delete_kubernetes() {
            panic!("{:?}", err)
        }
        let ret = tx.commit();
        assert!(matches!(ret, TransactionResult::Ok));

        test_name.to_string()
    });
}

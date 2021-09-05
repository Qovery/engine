extern crate test_utilities;

use self::test_utilities::cloudflare::dns_provider_cloudflare;
use self::test_utilities::utilities::{context, engine_run_test, init, FuncTestsSecrets};
use ::function_name::named;
use tracing::{span, Level};

use qovery_engine::cloud_provider::scaleway::kubernetes::node::NodeType;
use qovery_engine::cloud_provider::scaleway::kubernetes::Kapsule;
use qovery_engine::transaction::TransactionResult;

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

        let context = context();
        let engine = test_utilities::scaleway::docker_scw_cr_engine(&context);
        let session = engine.session().unwrap();
        let mut tx = session.transaction();

        let scw_cluster = test_utilities::scaleway::cloud_provider_scaleway(&context);
        let nodes = test_utilities::scaleway::scw_kubernetes_custom_nodes(10, NodeType::Gp1S);
        let cloudflare = dns_provider_cloudflare(&context);

        let kubernetes = Kapsule::new(
            context.clone(),
            test_utilities::scaleway::SCW_TEST_CLUSTER_ID.to_string(),
            test_utilities::scaleway::SCW_TEST_CLUSTER_NAME.to_string(),
            test_utilities::scaleway::SCW_KUBERNETES_VERSION.to_string(),
            test_utilities::scaleway::SCW_TEST_ZONE,
            &scw_cluster,
            &cloudflare,
            nodes,
            test_utilities::scaleway::scw_kubernetes_cluster_options(secrets),
        );

        // Deploy
        if let Err(err) = tx.create_kubernetes(&kubernetes) {
            panic!("{:?}", err)
        }
        let _ = match tx.commit() {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

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

        let context = context();
        let engine = test_utilities::scaleway::docker_scw_cr_engine(&context);
        let session = engine.session().unwrap();
        let mut tx = session.transaction();

        let scw_cluster = test_utilities::scaleway::cloud_provider_scaleway(&context);
        let nodes = test_utilities::scaleway::scw_kubernetes_nodes();
        let cloudflare = dns_provider_cloudflare(&context);

        let kubernetes = Kapsule::new(
            context.clone(),
            test_utilities::scaleway::SCW_TEST_CLUSTER_ID.to_string(),
            test_utilities::scaleway::SCW_TEST_CLUSTER_NAME.to_string(),
            test_utilities::scaleway::SCW_KUBERNETES_VERSION.to_string(),
            test_utilities::scaleway::SCW_TEST_ZONE,
            &scw_cluster,
            &cloudflare,
            nodes,
            test_utilities::scaleway::scw_kubernetes_cluster_options(secrets),
        );

        // Destroy
        if let Err(err) = tx.delete_kubernetes(&kubernetes) {
            panic!("{:?}", err)
        }
        match tx.commit() {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        test_name.to_string()
    });
}

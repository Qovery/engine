extern crate test_utilities;

use self::test_utilities::cloudflare::dns_provider_cloudflare;
use self::test_utilities::utilities::{context, engine_run_test, init, logger, FuncTestsSecrets};
use ::function_name::named;
use tracing::{span, Level};

use self::test_utilities::common::{Cluster, ClusterDomain};
use qovery_engine::cloud_provider::scaleway::kubernetes::Kapsule;
use qovery_engine::cloud_provider::scaleway::Scaleway;
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

        let logger = logger();
        let context = context();
        let engine = Scaleway::docker_cr_engine(&context, logger.clone());
        let session = engine.session().unwrap();
        let mut tx = session.transaction();

        let scw_cluster = Scaleway::cloud_provider(&context);
        let nodes = Scaleway::kubernetes_nodes();
        let cloudflare = dns_provider_cloudflare(&context, ClusterDomain::Default);

        let cluster_id = secrets
            .SCALEWAY_TEST_CLUSTER_ID
            .as_ref()
            .expect("SCALEWAY_TEST_CLUSTER_ID");

        let kubernetes = Kapsule::new(
            context.clone(),
            cluster_id.to_string(),
            uuid::Uuid::new_v4(),
            format!("qovery-{}", cluster_id.to_string()),
            test_utilities::scaleway::SCW_KUBERNETES_VERSION.to_string(),
            test_utilities::scaleway::SCW_TEST_ZONE,
            scw_cluster.as_ref(),
            &cloudflare,
            nodes,
            Scaleway::kubernetes_cluster_options(secrets, None),
            logger.as_ref(),
        )
        .unwrap();

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

        let logger = logger();
        let context = context();
        let engine = Scaleway::docker_cr_engine(&context, logger.clone());
        let session = engine.session().unwrap();
        let mut tx = session.transaction();

        let scw_cluster = Scaleway::cloud_provider(&context);
        let nodes = Scaleway::kubernetes_nodes();
        let cloudflare = dns_provider_cloudflare(&context, ClusterDomain::Default);

        let cluster_id = secrets
            .SCALEWAY_TEST_CLUSTER_ID
            .as_ref()
            .expect("SCALEWAY_TEST_CLUSTER_ID is not set");

        let kubernetes = Kapsule::new(
            context.clone(),
            cluster_id.to_string(),
            uuid::Uuid::new_v4(),
            format!("qovery-{}", cluster_id.to_string()),
            test_utilities::scaleway::SCW_KUBERNETES_VERSION.to_string(),
            test_utilities::scaleway::SCW_TEST_ZONE,
            scw_cluster.as_ref(),
            &cloudflare,
            nodes,
            Scaleway::kubernetes_cluster_options(secrets, None),
            logger.as_ref(),
        )
        .unwrap();

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

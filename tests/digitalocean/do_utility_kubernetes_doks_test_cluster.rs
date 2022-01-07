extern crate test_utilities;

use self::test_utilities::cloudflare::dns_provider_cloudflare;
use self::test_utilities::utilities::{context, engine_run_test, init, logger, FuncTestsSecrets};
use ::function_name::named;
use qovery_engine::cloud_provider::digitalocean::DO;
use tracing::{span, Level};

use self::test_utilities::common::{Cluster, ClusterDomain};
use qovery_engine::cloud_provider::digitalocean::kubernetes::DOKS;
use qovery_engine::transaction::TransactionResult;

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
        let cluster_name = format!("qovery-{}", cluster_id.clone());

        let logger = logger();
        let context = context(organization_id.as_str(), cluster_id.as_str());
        let engine = DO::docker_cr_engine(&context, logger.clone());
        let session = engine.session().unwrap();
        let mut tx = session.transaction();

        let do_cluster = DO::cloud_provider(&context);
        let nodes = DO::kubernetes_nodes();
        let cloudflare = dns_provider_cloudflare(&context, ClusterDomain::Default);

        let kubernetes = DOKS::new(
            context.clone(),
            cluster_id.to_string(),
            uuid::Uuid::new_v4(),
            cluster_name.to_string(),
            test_utilities::digitalocean::DO_KUBERNETES_VERSION.to_string(),
            test_utilities::digitalocean::DO_TEST_REGION,
            do_cluster.as_ref(),
            &cloudflare,
            nodes,
            DO::kubernetes_cluster_options(secrets, Option::from(cluster_name.to_string())),
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
        let cluster_name = format!("qovery-{}", cluster_id.clone());

        let logger = logger();
        let context = context(organization_id.as_str(), cluster_id.as_str());
        let engine = DO::docker_cr_engine(&context, logger.clone());
        let session = engine.session().unwrap();
        let mut tx = session.transaction();

        let do_cluster = DO::cloud_provider(&context);
        let nodes = DO::kubernetes_nodes();
        let cloudflare = dns_provider_cloudflare(&context, ClusterDomain::Default);

        let kubernetes = DOKS::new(
            context.clone(),
            cluster_id.to_string(),
            uuid::Uuid::new_v4(),
            cluster_name.to_string(),
            test_utilities::digitalocean::DO_KUBERNETES_VERSION.to_string(),
            test_utilities::digitalocean::DO_TEST_REGION,
            do_cluster.as_ref(),
            &cloudflare,
            nodes,
            DO::kubernetes_cluster_options(secrets, Option::from(cluster_name.to_string())),
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

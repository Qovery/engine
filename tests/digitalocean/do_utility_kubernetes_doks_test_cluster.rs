pub use ::function_name::named;
pub use tracing::{span, Level};

pub use crate::helpers::helpers_cloudflare::dns_provider_cloudflare;
pub use crate::helpers::helpers_digitalocean::{
    cloud_provider_digitalocean, do_kubernetes_cluster_options, do_kubernetes_nodes, docker_cr_do_engine,
    DO_KUBERNETES_VERSION, DO_KUBE_TEST_CLUSTER_ID, DO_KUBE_TEST_CLUSTER_NAME, DO_TEST_REGION,
};
pub use crate::helpers::utilities::{context, engine_run_test, init, FuncTestsSecrets};
pub use qovery_engine::cloud_provider::digitalocean::kubernetes::DOKS;
pub use qovery_engine::transaction::TransactionResult;

// Warning: This test shouldn't be ran by CI
// Note: this test creates the test cluster where all application tests will be ran
// This is not really a test but a convenient way to create the test cluster if needed to be manually created at some point.
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

        let context = context();
        let engine = docker_cr_do_engine(&context);
        let session = engine.session().unwrap();
        let mut tx = session.transaction();

        let do_cluster = cloud_provider_digitalocean(&context);
        let nodes = do_kubernetes_nodes();
        let cloudflare = dns_provider_cloudflare(&context);

        let kubernetes = DOKS::new(
            context,
            DO_KUBE_TEST_CLUSTER_ID.to_string(),
            uuid::Uuid::new_v4(),
            DO_KUBE_TEST_CLUSTER_NAME.to_string(),
            DO_KUBERNETES_VERSION.to_string(),
            DO_TEST_REGION,
            &do_cluster,
            &cloudflare,
            nodes,
            do_kubernetes_cluster_options(secrets, DO_KUBE_TEST_CLUSTER_NAME.to_string()),
        )
        .unwrap();

        // Deploy
        if let Err(err) = tx.create_kubernetes(&kubernetes) {
            panic!("{:?}", err)
        }
        let _ = match tx.commit() {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        test_name.to_string()
    });
}

// Warning: This test shouldn't be ran by CI
// Note: this test destroys the test cluster where all application tests will be ran
// This is not really a test but a convenient way to create the test cluster if needed to be manually destroyed at some point.
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

        let context = context();
        let engine = docker_cr_do_engine(&context);
        let session = engine.session().unwrap();
        let mut tx = session.transaction();

        let do_cluster = cloud_provider_digitalocean(&context);
        let nodes = do_kubernetes_nodes();
        let cloudflare = dns_provider_cloudflare(&context);

        let kubernetes = DOKS::new(
            context,
            DO_KUBE_TEST_CLUSTER_ID.to_string(),
            uuid::Uuid::new_v4(),
            DO_KUBE_TEST_CLUSTER_NAME.to_string(),
            DO_KUBERNETES_VERSION.to_string(),
            DO_TEST_REGION,
            &do_cluster,
            &cloudflare,
            nodes,
            do_kubernetes_cluster_options(secrets, DO_KUBE_TEST_CLUSTER_NAME.to_string()),
        )
        .unwrap();

        // Destroy
        if let Err(err) = tx.delete_kubernetes(&kubernetes) {
            panic!("{:?}", err)
        }
        match tx.commit() {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        test_name.to_string()
    });
}

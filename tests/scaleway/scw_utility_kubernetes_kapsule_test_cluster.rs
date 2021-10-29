pub use ::function_name::named;
pub use tracing::{span, Level};

pub use crate::helpers::cloudflare::dns_provider_cloudflare;
pub use crate::helpers::scaleway::{
    cloud_provider_scaleway, docker_scw_cr_engine, scw_kubernetes_cluster_options, scw_kubernetes_nodes,
    SCW_KUBERNETES_VERSION, SCW_KUBE_TEST_CLUSTER_ID, SCW_KUBE_TEST_CLUSTER_NAME, SCW_TEST_ZONE,
};
pub use crate::helpers::utilities::{context, engine_run_test, init, FuncTestsSecrets};
pub use qovery_engine::cloud_provider::scaleway::kubernetes::Kapsule;
pub use qovery_engine::transaction::TransactionResult;

// Warning: This test shouldn't be ran by CI
// Note: this test creates the test cluster where all application tests will be ran
// This is not really a test but a convenient way to create the test cluster if needed to be manually created at some point.
#[named]
#[test]
#[ignore]
#[cfg(test)]
fn create_scaleway_kubernetes_kapsule_test_cluster() {
    let secrets = FuncTestsSecrets::new();
    let test_name = function_name!();

    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "utility", name = test_name);
        let _enter = span.enter();

        let context = context();
        let engine = docker_scw_cr_engine(&context);
        let session = engine.session().unwrap();
        let mut tx = session.transaction();

        let scw_cluster = cloud_provider_scaleway(&context);
        let nodes = scw_kubernetes_nodes();
        let cloudflare = dns_provider_cloudflare(&context);

        let kubernetes = Kapsule::new(
            context,
            SCW_KUBE_TEST_CLUSTER_ID.to_string(),
            uuid::Uuid::new_v4(),
            SCW_KUBE_TEST_CLUSTER_NAME.to_string(),
            SCW_KUBERNETES_VERSION.to_string(),
            SCW_TEST_ZONE,
            &scw_cluster,
            &cloudflare,
            nodes,
            scw_kubernetes_cluster_options(secrets),
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
#[cfg(test)]
fn destroy_scaleway_kubernetes_kapsule_test_cluster() {
    let secrets = FuncTestsSecrets::new();
    let test_name = function_name!();

    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "utility", name = test_name);
        let _enter = span.enter();

        let context = context();
        let engine = docker_scw_cr_engine(&context);
        let session = engine.session().unwrap();
        let mut tx = session.transaction();

        let scw_cluster = cloud_provider_scaleway(&context);
        let nodes = scw_kubernetes_nodes();
        let cloudflare = dns_provider_cloudflare(&context);

        let kubernetes = Kapsule::new(
            context,
            SCW_KUBE_TEST_CLUSTER_ID.to_string(),
            uuid::Uuid::new_v4(),
            SCW_KUBE_TEST_CLUSTER_NAME.to_string(),
            SCW_KUBERNETES_VERSION.to_string(),
            SCW_TEST_ZONE,
            &scw_cluster,
            &cloudflare,
            nodes,
            scw_kubernetes_cluster_options(secrets),
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

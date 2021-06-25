extern crate test_utilities;

use self::test_utilities::cloudflare::dns_provider_cloudflare;
use self::test_utilities::utilities::{
    context, engine_run_test, generate_cluster_id, generate_id, init, FuncTestsSecrets,
};
use std::env;
use test_utilities::aws::AWS_KUBERNETES_VERSION;
use tracing::{span, Level};

use qovery_engine::transaction::TransactionResult;

fn create_upgrade_and_destroy_kubernetes_cluster(
    region: &str,
    secrets: FuncTestsSecrets,
    boot_version: &str,
    upgrade_to_version: &str,
    test_name: &str,
) {
    // TODO(benjaminch): Implement it

    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let context = context();
        let engine = test_utilities::aws::docker_ecr_aws_engine(&context);
        let session = engine.session().unwrap();
        let mut tx = session.transaction();

        // Deploy

        // Upgrade

        // Destroy

        test_name.to_string()
    });
}

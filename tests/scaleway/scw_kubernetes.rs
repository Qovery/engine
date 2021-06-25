extern crate test_utilities;

use self::test_utilities::cloudflare::dns_provider_cloudflare;
use self::test_utilities::utilities::{context, engine_run_test, generate_id, generate_cluster_id, init, FuncTestsSecrets};
use std::env;
use test_utilities::aws::AWS_KUBERNETES_VERSION;
use tracing::{span, Level};

use qovery_engine::cloud_provider::aws::kubernetes::EKS;
use qovery_engine::transaction::TransactionResult;

#[allow(dead_code)]
fn create_upgrade_and_destroy_kubernetes_cluster(
    region: &str,
    secrets: FuncTestsSecrets,
    boot_version: &str,
    upgrade_to_version: &str,
    test_name: &str,
) {
    // TODO(benjaminch): Implement it
}

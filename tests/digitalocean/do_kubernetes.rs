extern crate test_utilities;

use self::test_utilities::cloudflare::dns_provider_cloudflare;
use self::test_utilities::utilities::{
    cluster_test, context, engine_run_test, generate_cluster_id, init, FuncTestsSecrets,
};
use ::function_name::named;
use tracing::{span, Level};

use qovery_engine::cloud_provider::digitalocean::kubernetes::{DoksOptions, DOKS};
use qovery_engine::transaction::TransactionResult;

use self::test_utilities::digitalocean::{DO_KUBERNETES_MAJOR_VERSION, DO_KUBERNETES_MINOR_VERSION};
use qovery_engine::cloud_provider::digitalocean::application::Region;
use qovery_engine::cloud_provider::digitalocean::DO;
use qovery_engine::cloud_provider::Kind;
use test_utilities::digitalocean::DO_KUBERNETES_VERSION;

#[allow(dead_code)]
fn create_and_destroy_doks_cluster(
    region: Region,
    secrets: FuncTestsSecrets,
    test_infra_pause: bool,
    test_infra_upgrade: bool,
    major_boot_version: u8,
    minor_boot_version: u8,
    test_name: &str,
) {
    engine_run_test(|| {
        cluster_test(
            test_name,
            Kind::Do,
            region.as_str(),
            secrets,
            test_infra_pause,
            test_infra_upgrade,
            major_boot_version,
            minor_boot_version,
            None,
        )
    })
}

#[cfg(feature = "test-do-infra")]
#[named]
#[test]
fn create_and_destroy_doks_cluster_ams_3() {
    let region = Region::Amsterdam3;
    let secrets = FuncTestsSecrets::new();
    create_and_destroy_doks_cluster(
        region,
        secrets,
        false,
        false,
        DO_KUBERNETES_MAJOR_VERSION,
        DO_KUBERNETES_MINOR_VERSION,
        function_name!(),
    );
}

#[cfg(feature = "test-do-infra")]
#[named]
#[test]
fn create_upgrade_and_destroy_doks_cluster_in_nyc_3() {
    let region = Region::NewYorkCity3;
    let secrets = FuncTestsSecrets::new();
    create_and_destroy_doks_cluster(
        region,
        secrets,
        false,
        true,
        DO_KUBERNETES_MAJOR_VERSION,
        DO_KUBERNETES_MINOR_VERSION,
        function_name!(),
    );
}

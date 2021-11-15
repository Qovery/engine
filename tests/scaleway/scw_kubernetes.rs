extern crate test_utilities;

use self::test_utilities::cloudflare::dns_provider_cloudflare;
use self::test_utilities::utilities::{
    cluster_test, context, engine_run_test, generate_cluster_id, init, FuncTestsSecrets,
};
use ::function_name::named;
use tracing::{span, Level};

use qovery_engine::cloud_provider::scaleway::application::Zone;
use qovery_engine::cloud_provider::scaleway::kubernetes::{Kapsule, KapsuleOptions};
use qovery_engine::transaction::TransactionResult;

use self::test_utilities::scaleway::{SCW_KUBERNETES_MAJOR_VERSION, SCW_KUBERNETES_MINOR_VERSION};
use qovery_engine::cloud_provider::scaleway::Scaleway;
use qovery_engine::cloud_provider::Kind;
use test_utilities::scaleway::SCW_KUBERNETES_VERSION;

#[allow(dead_code)]
fn create_and_destroy_kapsule_cluster(
    zone: Zone,
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
            Kind::Scw,
            zone.as_str(),
            secrets,
            test_infra_pause,
            test_infra_upgrade,
            major_boot_version,
            minor_boot_version,
            Option::from(vpc_network_mode),
        )
    })
}

#[cfg(feature = "test-scw-infra")]
#[named]
#[ignore]
#[test]
fn create_and_destroy_kapsule_cluster_par_1() {
    let zone = Zone::Paris1;
    let secrets = FuncTestsSecrets::new();
    create_and_destroy_kapsule_cluster(
        zone,
        secrets,
        false,
        false,
        SCW_KUBERNETES_MAJOR_VERSION,
        SCW_KUBERNETES_MINOR_VERSION,
        function_name!(),
    );
}

#[cfg(feature = "test-scw-infra")]
#[named]
#[test]
fn create_and_destroy_kapsule_cluster_par_2() {
    let zone = Zone::Paris2;
    let secrets = FuncTestsSecrets::new();
    create_and_destroy_kapsule_cluster(
        zone,
        secrets,
        false,
        false,
        SCW_KUBERNETES_MAJOR_VERSION,
        SCW_KUBERNETES_MINOR_VERSION,
        function_name!(),
    );
}

#[cfg(feature = "test-scw-infra")]
#[named]
#[test]
#[ignore]
fn create_and_destroy_kapsule_cluster_ams_1() {
    let zone = Zone::Amsterdam1;
    let secrets = FuncTestsSecrets::new();
    create_and_destroy_kapsule_cluster(
        zone,
        secrets,
        false,
        false,
        SCW_KUBERNETES_MAJOR_VERSION,
        SCW_KUBERNETES_MINOR_VERSION,
        function_name!(),
    );
}

#[cfg(feature = "test-scw-infra")]
#[named]
#[test]
fn create_and_destroy_kapsule_cluster_war_1() {
    let zone = Zone::Warsaw1;
    let secrets = FuncTestsSecrets::new();
    create_and_destroy_kapsule_cluster(
        zone,
        secrets,
        false,
        false,
        SCW_KUBERNETES_MAJOR_VERSION,
        SCW_KUBERNETES_MINOR_VERSION,
        function_name!(),
    );
}

// only enable this test manually when we want to perform and validate upgrade process
#[test]
#[ignore]
#[named]
fn create_upgrade_and_destroy_kapsule_cluster_in_par_1() {
    let zone = Zone::Paris1;
    let secrets = FuncTestsSecrets::new();
    create_and_destroy_kapsule_cluster(
        zone,
        secrets,
        false,
        true,
        SCW_KUBERNETES_MAJOR_VERSION,
        SCW_KUBERNETES_MINOR_VERSION,
        function_name!(),
    );
}

// only enable this test manually when we want to perform and validate upgrade process
#[test]
#[ignore]
#[named]
fn create_upgrade_and_destroy_kapsule_cluster_in_par_2() {
    let zone = Zone::Paris2;
    let secrets = FuncTestsSecrets::new();
    create_and_destroy_kapsule_cluster(
        zone,
        secrets,
        false,
        true,
        SCW_KUBERNETES_MAJOR_VERSION,
        SCW_KUBERNETES_MINOR_VERSION,
        function_name!(),
    );
}

// only enable this test manually when we want to perform and validate upgrade process
#[test]
#[ignore]
#[named]
fn create_upgrade_and_destroy_kapsule_cluster_in_ams_1() {
    let zone = Zone::Amsterdam1;
    let secrets = FuncTestsSecrets::new();
    create_and_destroy_kapsule_cluster(
        zone,
        secrets,
        false,
        true,
        SCW_KUBERNETES_MAJOR_VERSION,
        SCW_KUBERNETES_MINOR_VERSION,
        function_name!(),
    );
}

// only enable this test manually when we want to perform and validate upgrade process
#[test]
#[ignore]
#[named]
fn create_upgrade_and_destroy_kapsule_cluster_in_war_1() {
    let zone = Zone::Warsaw1;
    let secrets = FuncTestsSecrets::new();
    create_and_destroy_kapsule_cluster(
        zone,
        secrets,
        false,
        true,
        SCW_KUBERNETES_MAJOR_VERSION,
        SCW_KUBERNETES_MINOR_VERSION,
        function_name!(),
    );
}

extern crate test_utilities;

use self::test_utilities::scaleway::{SCW_KUBERNETES_MAJOR_VERSION, SCW_KUBERNETES_MINOR_VERSION};
use self::test_utilities::utilities::{
    cluster_test, context, engine_run_test, generate_cluster_id, init, FuncTestsSecrets,
};
use ::function_name::named;
use qovery_engine::cloud_provider::aws::kubernetes::VpcQoveryNetworkMode;
use qovery_engine::cloud_provider::scaleway::application::Zone;
use qovery_engine::cloud_provider::Kind;

#[cfg(feature = "test-scw-infra")]
fn create_and_destroy_kapsule_cluster(
    zone: Zone,
    secrets: FuncTestsSecrets,
    test_infra_pause: bool,
    test_infra_upgrade: bool,
    major_boot_version: u8,
    minor_boot_version: u8,
    test_name: &str,
    vpc_network_mode: Option<VpcQoveryNetworkMode>,
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
            vpc_network_mode,
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
        None,
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
        None,
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
        None,
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
        None,
    );
}

// only enable this test manually when we want to perform and validate upgrade process
#[cfg(feature = "test-scw-infra")]
#[test]
#[named]
#[ignore]
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
        None,
    );
}

// only enable this test manually when we want to perform and validate upgrade process
#[cfg(feature = "test-scw-infra")]
#[test]
#[named]
#[ignore]
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
        None,
    );
}

// only enable this test manually when we want to perform and validate upgrade process
#[cfg(feature = "test-scw-infra")]
#[test]
#[named]
#[ignore]
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
        None,
    );
}

// only enable this test manually when we want to perform and validate upgrade process
#[cfg(feature = "test-scw-infra")]
#[test]
#[named]
#[ignore]
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
        None,
    );
}

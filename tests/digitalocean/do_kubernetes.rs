extern crate test_utilities;

use self::test_utilities::common::ClusterDomain;
use self::test_utilities::digitalocean::{DO_KUBERNETES_MAJOR_VERSION, DO_KUBERNETES_MINOR_VERSION};
use self::test_utilities::utilities::{context, engine_run_test, FuncTestsSecrets};
use ::function_name::named;
use qovery_engine::cloud_provider::digitalocean::application::Region;
use qovery_engine::cloud_provider::Kind;
use test_utilities::common::{cluster_test, ClusterTestType};

#[cfg(feature = "test-do-infra")]
fn create_and_destroy_doks_cluster(
    region: Region,
    secrets: FuncTestsSecrets,
    test_type: ClusterTestType,
    major_boot_version: u8,
    minor_boot_version: u8,
    test_name: &str,
) {
    engine_run_test(|| {
        cluster_test(
            test_name,
            Kind::Do,
            context(),
            region.as_str(),
            secrets,
            test_type,
            major_boot_version,
            minor_boot_version,
            ClusterDomain::Default,
            None,
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
        ClusterTestType::Classic,
        DO_KUBERNETES_MAJOR_VERSION,
        DO_KUBERNETES_MINOR_VERSION,
        function_name!(),
    );
}

#[cfg(feature = "test-do-infra")]
#[named]
#[test]
#[ignore]
fn create_upgrade_and_destroy_doks_cluster_in_nyc_3() {
    let region = Region::NewYorkCity3;
    let secrets = FuncTestsSecrets::new();
    create_and_destroy_doks_cluster(
        region,
        secrets,
        ClusterTestType::Classic,
        DO_KUBERNETES_MAJOR_VERSION,
        DO_KUBERNETES_MINOR_VERSION,
        function_name!(),
    );
}

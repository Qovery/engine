extern crate test_utilities;

use self::test_utilities::common::ClusterDomain;
use self::test_utilities::digitalocean::{DO_KUBERNETES_MAJOR_VERSION, DO_KUBERNETES_MINOR_VERSION};
use self::test_utilities::utilities::{
    context, engine_run_test, generate_cluster_id, generate_id, logger, FuncTestsSecrets,
};
use ::function_name::named;
use qovery_engine::cloud_provider::digitalocean::application::DoRegion;
use qovery_engine::cloud_provider::Kind;
use test_utilities::common::{cluster_test, ClusterTestType};

#[cfg(feature = "test-do-infra")]
fn create_and_destroy_doks_cluster(
    region: DoRegion,
    test_type: ClusterTestType,
    major_boot_version: u8,
    minor_boot_version: u8,
    test_name: &str,
) {
    engine_run_test(|| {
        cluster_test(
            test_name,
            Kind::Do,
            context(generate_id().as_str(), generate_cluster_id(region.as_str()).as_str()),
            logger(),
            region.as_str(),
            None,
            test_type,
            major_boot_version,
            minor_boot_version,
            &ClusterDomain::Default,
            None,
            None,
        )
    })
}

#[cfg(feature = "test-do-infra")]
#[named]
#[test]
fn create_and_destroy_doks_cluster_ams_3() {
    let region = DoRegion::Amsterdam3;
    create_and_destroy_doks_cluster(
        region,
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
    let region = DoRegion::NewYorkCity3;
    create_and_destroy_doks_cluster(
        region,
        ClusterTestType::WithUpgrade,
        DO_KUBERNETES_MAJOR_VERSION,
        DO_KUBERNETES_MINOR_VERSION,
        function_name!(),
    );
}

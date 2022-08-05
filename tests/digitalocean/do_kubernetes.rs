use crate::helpers::common::ClusterDomain;
use crate::helpers::digitalocean::{DO_KUBERNETES_MAJOR_VERSION, DO_KUBERNETES_MINOR_VERSION};
use crate::helpers::kubernetes::{cluster_test, ClusterTestType};
use crate::helpers::utilities::{context, engine_run_test, generate_cluster_id, generate_id, logger};
use ::function_name::named;
use qovery_engine::cloud_provider::aws::kubernetes::VpcQoveryNetworkMode;
use qovery_engine::cloud_provider::kubernetes::Kind as KKind;
use qovery_engine::cloud_provider::Kind;
use qovery_engine::models::digital_ocean::DoRegion;

#[cfg(feature = "test-do-infra")]
fn create_and_destroy_doks_cluster(
    region: DoRegion,
    test_type: ClusterTestType,
    major_boot_version: u8,
    minor_boot_version: u8,
    test_name: &str,
    vpc_network_mode: Option<VpcQoveryNetworkMode>,
) {
    engine_run_test(|| {
        let cluster_id = generate_cluster_id(region.as_str());
        cluster_test(
            test_name,
            Kind::Do,
            KKind::Doks,
            context(generate_id().as_str(), cluster_id.as_str()),
            logger(),
            region.as_str(),
            None,
            test_type,
            major_boot_version,
            minor_boot_version,
            &ClusterDomain::Default { cluster_id },
            vpc_network_mode,
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
        None,
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
        None,
    );
}

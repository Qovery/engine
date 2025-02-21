use crate::helpers::utilities::{
    context_for_cluster, engine_run_test, generate_cluster_id, generate_organization_id, logger, metrics_registry,
};
use ::function_name::named;
use qovery_engine::infrastructure::models::kubernetes::Kind as KKind;

use crate::helpers::common::{ClusterDomain, NodeManager};
use crate::helpers::kubernetes::{ClusterTestType, cluster_test};
use qovery_engine::environment::models::scaleway::ScwZone;
use qovery_engine::infrastructure::models::cloud_provider::Kind;
use qovery_engine::io_models::models::{CpuArchitecture, VpcQoveryNetworkMode};
use qovery_engine::utilities::to_short_id;

#[cfg(any(feature = "test-scw-infra", feature = "test-scw-infra-upgrade"))]
fn create_and_destroy_kapsule_cluster(
    zone: ScwZone,
    test_type: ClusterTestType,
    test_name: &str,
    vpc_network_mode: Option<VpcQoveryNetworkMode>,
) {
    engine_run_test(|| {
        let cluster_id = generate_cluster_id(zone.as_str());
        let organization_id = generate_organization_id(zone.as_str());
        cluster_test(
            test_name,
            Kind::Scw,
            KKind::ScwKapsule,
            context_for_cluster(organization_id, cluster_id, None),
            logger(),
            metrics_registry(),
            zone.as_str(),
            None,
            test_type,
            &ClusterDomain::Default {
                cluster_id: to_short_id(&cluster_id),
            },
            vpc_network_mode,
            CpuArchitecture::AMD64,
            None,
            NodeManager::Default,
        )
    })
}

#[cfg(feature = "test-scw-infra")]
#[named]
#[ignore]
#[test]
fn create_and_destroy_kapsule_cluster_par_1() {
    let zone = ScwZone::Paris1;
    create_and_destroy_kapsule_cluster(zone, ClusterTestType::Classic, function_name!(), None);
}

#[cfg(feature = "test-scw-infra")]
#[named]
#[ignore]
#[test]
fn create_and_destroy_kapsule_cluster_par_2() {
    let zone = ScwZone::Paris2;
    create_and_destroy_kapsule_cluster(zone, ClusterTestType::Classic, function_name!(), None);
}

#[cfg(feature = "test-scw-infra")]
#[named]
#[test]
fn create_and_destroy_kapsule_cluster_waw() {
    let zone = ScwZone::Warsaw1;
    create_and_destroy_kapsule_cluster(zone, ClusterTestType::Classic, function_name!(), None);
}

#[cfg(feature = "test-scw-infra")]
#[named]
#[test]
fn create_pause_and_destroy_kapsule_cluster_ams_1() {
    let zone = ScwZone::Amsterdam1;
    create_and_destroy_kapsule_cluster(zone, ClusterTestType::WithPause, function_name!(), None);
}

#[cfg(feature = "test-scw-infra")]
#[named]
#[test]
fn create_and_destroy_kapsule_cluster_war_1() {
    let zone = ScwZone::Warsaw1;
    create_and_destroy_kapsule_cluster(zone, ClusterTestType::Classic, function_name!(), None);
}

// only enable this test manually when we want to perform and validate upgrade process
#[cfg(feature = "test-scw-infra-upgrade")]
#[test]
#[named]
fn create_upgrade_and_destroy_kapsule_cluster_in_par_1() {
    let zone = ScwZone::Paris1;
    create_and_destroy_kapsule_cluster(zone, ClusterTestType::WithUpgrade, function_name!(), None);
}

// only enable this test manually when we want to perform and validate upgrade process
#[cfg(feature = "test-scw-infra-upgrade")]
#[test]
#[named]
fn create_upgrade_and_destroy_kapsule_cluster_in_par_2() {
    let zone = ScwZone::Paris2;
    create_and_destroy_kapsule_cluster(zone, ClusterTestType::WithUpgrade, function_name!(), None);
}

// only enable this test manually when we want to perform and validate upgrade process
#[cfg(feature = "test-scw-infra-upgrade")]
#[test]
#[named]
fn create_upgrade_and_destroy_kapsule_cluster_in_ams_1() {
    let zone = ScwZone::Amsterdam1;
    create_and_destroy_kapsule_cluster(zone, ClusterTestType::WithUpgrade, function_name!(), None);
}

// only enable this test manually when we want to perform and validate upgrade process
#[cfg(feature = "test-scw-infra-upgrade")]
#[test]
#[named]
fn create_upgrade_and_destroy_kapsule_cluster_in_war_1() {
    let zone = ScwZone::Warsaw1;
    create_and_destroy_kapsule_cluster(zone, ClusterTestType::WithUpgrade, function_name!(), None);
}

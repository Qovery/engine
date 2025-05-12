use crate::helpers::common::{ClusterDomain, NodeManager};
use crate::helpers::kubernetes::{ClusterTestType, cluster_test};
use crate::helpers::utilities::{
    context_for_cluster, engine_run_test, generate_cluster_id, generate_organization_id, logger, metrics_registry,
};
use ::function_name::named;
use qovery_engine::environment::models::ToCloudProviderFormat;
use qovery_engine::infrastructure::models::cloud_provider::Kind;
use qovery_engine::infrastructure::models::cloud_provider::gcp::locations::GcpRegion;
use qovery_engine::infrastructure::models::kubernetes::Kind as KKind;
use qovery_engine::io_models::models::{CpuArchitecture, VpcQoveryNetworkMode};
use qovery_engine::utilities::to_short_id;

#[cfg(any(feature = "test-gcp-infra", feature = "test-gcp-infra-upgrade"))]
fn create_and_destroy_gke_cluster(
    region: GcpRegion,
    test_type: ClusterTestType,
    test_name: &str,
    vpc_network_mode: Option<VpcQoveryNetworkMode>,
) {
    engine_run_test(|| {
        let cluster_id = generate_cluster_id(region.to_string().as_str());
        let organization_id = generate_organization_id(region.to_string().as_str());
        let zones = region.zones();
        cluster_test(
            test_name,
            Kind::Gcp,
            KKind::Gke,
            context_for_cluster(organization_id, cluster_id, Some(KKind::Gke)),
            logger(),
            metrics_registry(),
            region.to_cloud_provider_format(),
            Some(zones.iter().map(|z| z.to_cloud_provider_format()).collect()),
            test_type,
            &ClusterDomain::Default {
                cluster_id: to_short_id(&cluster_id),
            },
            vpc_network_mode,
            CpuArchitecture::AMD64,
            None,
            NodeManager::AutoPilot,
            vec![],
            // TODO (mzo)
        )
    })
}

#[cfg(feature = "test-gcp-infra")]
#[named]
#[test]
fn create_and_destroy_gke_cluster_in_europe_west_10() {
    let region = GcpRegion::EuropeWest10;
    create_and_destroy_gke_cluster(region, ClusterTestType::Classic, function_name!(), None);
}

#[cfg(feature = "test-gcp-infra")]
#[named]
#[test]
fn create_and_destroy_gke_cluster_with_nat_gateway_in_europe_west_12() {
    let region = GcpRegion::EuropeWest12;
    create_and_destroy_gke_cluster(
        region,
        ClusterTestType::Classic,
        function_name!(),
        Some(VpcQoveryNetworkMode::WithNatGateways),
    );
}

// only enable this test manually when we want to perform and validate upgrade process
#[cfg(feature = "test-gcp-infra-upgrade")]
#[named]
#[test]
fn create_upgrade_and_destroy_gke_cluster_in_europe_west_9() {
    let region = GcpRegion::EuropeWest9;
    create_and_destroy_gke_cluster(region, ClusterTestType::WithUpgrade, function_name!(), None);
}

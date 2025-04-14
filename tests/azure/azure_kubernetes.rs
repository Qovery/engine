use crate::helpers::azure::AZURE_LOCATION;
use crate::helpers::common::{ClusterDomain, NodeManager};
use crate::helpers::kubernetes::{ClusterTestType, cluster_test};
use crate::helpers::utilities::{
    context_for_cluster, engine_run_test, generate_cluster_id, generate_organization_id, logger, metrics_registry,
};
use function_name::named;
use qovery_engine::environment::models::ToCloudProviderFormat;
use qovery_engine::infrastructure::models::cloud_provider::Kind;
use qovery_engine::infrastructure::models::cloud_provider::azure::locations::AzureLocation;
use qovery_engine::infrastructure::models::kubernetes::Kind as KKind;
use qovery_engine::io_models::models::VpcQoveryNetworkMode::WithoutNatGateways;
use qovery_engine::io_models::models::{CpuArchitecture, VpcQoveryNetworkMode};
use qovery_engine::utilities::to_short_id;
use std::str::FromStr;

#[cfg(any(feature = "test-azure-infra", feature = "test-azure-infra-upgrade",))]
fn create_and_destroy_eks_cluster(
    region: String,
    test_type: ClusterTestType,
    vpc_network_mode: VpcQoveryNetworkMode,
    test_name: &str,
    node_manager: NodeManager,
) {
    engine_run_test(|| {
        let region = AzureLocation::from_str(region.as_str()).expect("Wasn't able to convert the desired region");
        let cluster_id = generate_cluster_id(region.to_string().as_str());
        let organization_id = generate_organization_id(region.to_string().as_str());
        cluster_test(
            test_name,
            Kind::Azure,
            KKind::Aks,
            context_for_cluster(organization_id, cluster_id, Some(KKind::Aks)),
            logger(),
            metrics_registry(),
            region.to_cloud_provider_format(),
            None,
            test_type,
            &ClusterDomain::Default {
                cluster_id: to_short_id(&cluster_id),
            },
            Option::from(vpc_network_mode),
            CpuArchitecture::AMD64,
            None,
            node_manager,
        )
    })
}

#[cfg(feature = "test-azure-infra")]
#[named]
#[test]
fn create_and_destroy_aks_cluster_francecentral() {
    let region = AZURE_LOCATION.to_cloud_provider_format().to_string();
    create_and_destroy_eks_cluster(
        region,
        ClusterTestType::Classic,
        WithoutNatGateways,
        function_name!(),
        NodeManager::Default,
    );
}

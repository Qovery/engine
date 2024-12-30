use std::str::FromStr;

use crate::helpers::common::ClusterDomain;
use crate::helpers::utilities::{
    context_for_cluster, engine_run_test, generate_cluster_id, generate_organization_id, logger, metrics_registry,
};
use ::function_name::named;

use crate::helpers::kubernetes::{cluster_test, ClusterTestType};
use qovery_engine::environment::models::ToCloudProviderFormat;
use qovery_engine::infrastructure::models::cloud_provider::aws::regions::AwsRegion;
use qovery_engine::infrastructure::models::cloud_provider::Kind;
use qovery_engine::infrastructure::models::kubernetes::Kind as KKind;
use qovery_engine::io_models::models::VpcQoveryNetworkMode::{WithNatGateways, WithoutNatGateways};
use qovery_engine::io_models::models::{CpuArchitecture, VpcQoveryNetworkMode};
use qovery_engine::utilities::to_short_id;

#[cfg(any(
    feature = "test-aws-infra",
    feature = "test-aws-infra-arm",
    feature = "test-aws-infra-nat-gateway",
    feature = "test-aws-infra-upgrade"
))]
fn create_and_destroy_eks_cluster(
    region: String,
    test_type: ClusterTestType,
    vpc_network_mode: VpcQoveryNetworkMode,
    test_name: &str,
) {
    engine_run_test(|| {
        let region = AwsRegion::from_str(region.as_str()).expect("Wasn't able to convert the desired region");
        let cluster_id = generate_cluster_id(region.to_string().as_str());
        let organization_id = generate_organization_id(region.to_string().as_str());
        let zones = region.zones();
        cluster_test(
            test_name,
            Kind::Aws,
            KKind::Eks,
            context_for_cluster(organization_id, cluster_id, Some(KKind::Eks)),
            logger(),
            metrics_registry(),
            region.to_cloud_provider_format(),
            Some(zones.iter().map(|z| z.to_cloud_provider_format()).collect()),
            test_type,
            &ClusterDomain::Default {
                cluster_id: to_short_id(&cluster_id),
            },
            Option::from(vpc_network_mode),
            CpuArchitecture::AMD64,
            None,
        )
    })
}

#[cfg(any(feature = "test-aws-infra-arm", feature = "test-aws-infra-upgrade"))]
fn create_and_destroy_arm64_eks_cluster(
    region: String,
    test_type: ClusterTestType,
    vpc_network_mode: VpcQoveryNetworkMode,
    test_name: &str,
) {
    engine_run_test(|| {
        let region = AwsRegion::from_str(region.as_str()).expect("Wasn't able to convert the desired region");
        let cluster_id = generate_cluster_id(region.to_string().as_str());
        let organization_id = generate_organization_id(region.to_string().as_str());
        let zones = region.zones();
        cluster_test(
            test_name,
            Kind::Aws,
            KKind::Eks,
            context_for_cluster(organization_id, cluster_id, Some(KKind::Eks)),
            logger(),
            metrics_registry(),
            region.to_cloud_provider_format(),
            Some(zones.iter().map(|z| z.to_cloud_provider_format()).collect()),
            test_type,
            &ClusterDomain::Default {
                cluster_id: to_short_id(&cluster_id),
            },
            Option::from(vpc_network_mode),
            CpuArchitecture::ARM64,
            None,
        )
    })
}
/*
    TESTS NOTES:
    It is useful to keep 2 clusters deployment tests to run in // to validate there is no name collision (overlaping)
*/

// x86-64

#[cfg(feature = "test-aws-infra")]
#[named]
#[test]
fn create_and_destroy_eks_cluster_without_nat_gw_in_eu_west_3() {
    let region = "eu-west-3".to_string();
    create_and_destroy_eks_cluster(region, ClusterTestType::Classic, WithoutNatGateways, function_name!());
}

#[cfg(feature = "test-aws-infra-nat-gateway")]
#[named]
#[test]
fn create_and_destroy_eks_cluster_with_nat_gw_in_us_east_2() {
    let region = "us-east-2".to_string();
    create_and_destroy_eks_cluster(region, ClusterTestType::Classic, WithNatGateways, function_name!());
}

#[cfg(feature = "test-aws-infra")]
#[ignore]
#[named]
#[test]
fn create_and_destroy_eks_cluster_in_us_east_2() {
    let region = "us-east-2".to_string();
    create_and_destroy_eks_cluster(region, ClusterTestType::Classic, WithoutNatGateways, function_name!());
}

#[cfg(feature = "test-aws-infra")]
#[named]
#[test]
fn create_pause_and_destroy_eks_cluster_in_us_east_2() {
    let region = "us-east-2".to_string();
    create_and_destroy_eks_cluster(region, ClusterTestType::WithPause, WithoutNatGateways, function_name!());
}

// only enable this test manually when we want to perform and validate upgrade process
#[cfg(feature = "test-aws-infra-upgrade")]
#[named]
#[test]
fn create_upgrade_and_destroy_eks_cluster_in_eu_west_3() {
    let region = "eu-west-3".to_string();
    create_and_destroy_eks_cluster(region, ClusterTestType::WithUpgrade, WithoutNatGateways, function_name!());
}

// ARM64

#[cfg(feature = "test-aws-infra-arm")]
#[named]
#[test]
fn create_and_destroy_eks_cluster_arm64_without_nat_gw_in_eu_west_3() {
    let region = "eu-west-3".to_string();
    create_and_destroy_arm64_eks_cluster(region, ClusterTestType::Classic, WithoutNatGateways, function_name!());
}

#[cfg(feature = "test-aws-infra-upgrade")]
#[named]
#[test]
fn create_upgrade_and_destroy_eks_cluster_arm64_without_nat_gw_in_eu_west_3() {
    let region = "eu-west-3".to_string();
    create_and_destroy_arm64_eks_cluster(region, ClusterTestType::WithUpgrade, WithoutNatGateways, function_name!());
}

// Karpenter

#[cfg(feature = "test-aws-infra-karpenter")]
#[test]
#[ignore = "TODO(ENG-1888): To be implemented"]
fn create_and_destroy_eks_cluster_karpenter_without_nat_gw_in_eu_west_3() {}

#[cfg(feature = "test-aws-infra-upgrade")]
#[test]
#[ignore = "TODO(ENG-1888): To be implemented"]
fn create_upgrade_and_destroy_eks_cluster_karpenter_without_nat_gw_in_eu_west_3() {}

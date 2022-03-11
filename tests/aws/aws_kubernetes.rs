extern crate test_utilities;

use self::test_utilities::aws::{AWS_KUBERNETES_MAJOR_VERSION, AWS_KUBERNETES_MINOR_VERSION};
use self::test_utilities::utilities::{
    context, engine_run_test, generate_cluster_id, generate_id, logger, FuncTestsSecrets,
};
use ::function_name::named;
use qovery_engine::cloud_provider::aws::kubernetes::VpcQoveryNetworkMode;
use qovery_engine::cloud_provider::aws::kubernetes::VpcQoveryNetworkMode::{WithNatGateways, WithoutNatGateways};
use qovery_engine::cloud_provider::aws::regions::{AwsRegion, AwsZones};
use qovery_engine::cloud_provider::Kind;
use std::borrow::Borrow;
use std::str::FromStr;
use test_utilities::common::{cluster_test, ClusterDomain, ClusterTestType};

#[cfg(feature = "test-aws-infra")]
fn create_and_destroy_eks_cluster(
    region: String,
    test_type: ClusterTestType,
    major_boot_version: u8,
    minor_boot_version: u8,
    vpc_network_mode: VpcQoveryNetworkMode,
    test_name: &str,
) {
    engine_run_test(|| {
        let region = AwsRegion::from_str(region.as_str()).expect("Wasn't able to convert the desired region");
        let zones = region.get_zones();
        cluster_test(
            test_name,
            Kind::Aws,
            context(
                generate_id().as_str(),
                generate_cluster_id(region.to_string().as_str()).as_str(),
            ),
            logger(),
            region.to_aws_format().as_str(),
            Some(zones),
            test_type,
            major_boot_version,
            minor_boot_version,
            &ClusterDomain::Default,
            Option::from(vpc_network_mode),
            None,
        )
    })
}

/*
    TESTS NOTES:
    It is useful to keep 2 clusters deployment tests to run in // to validate there is no name collision (overlaping)
*/

#[cfg(feature = "test-aws-infra")]
#[named]
#[test]
fn create_and_destroy_eks_cluster_without_nat_gw_in_eu_west_3() {
    let region = "eu-west-3".to_string();
    create_and_destroy_eks_cluster(
        region,
        ClusterTestType::Classic,
        AWS_KUBERNETES_MAJOR_VERSION,
        AWS_KUBERNETES_MINOR_VERSION,
        WithoutNatGateways,
        function_name!(),
    );
}

#[cfg(feature = "test-aws-infra")]
#[named]
#[test]
fn create_and_destroy_eks_cluster_with_nat_gw_in_eu_west_3() {
    let region = "eu-west-3".to_string();
    create_and_destroy_eks_cluster(
        region,
        ClusterTestType::Classic,
        AWS_KUBERNETES_MAJOR_VERSION,
        AWS_KUBERNETES_MINOR_VERSION,
        WithNatGateways,
        function_name!(),
    );
}

#[cfg(feature = "test-aws-infra")]
#[named]
#[test]
fn create_and_destroy_eks_cluster_in_us_east_2() {
    let region = "us-east-2".to_string();
    create_and_destroy_eks_cluster(
        region,
        ClusterTestType::Classic,
        AWS_KUBERNETES_MAJOR_VERSION,
        AWS_KUBERNETES_MINOR_VERSION,
        WithoutNatGateways,
        function_name!(),
    );
}

#[cfg(feature = "test-aws-infra")]
#[named]
#[test]
fn create_pause_and_destroy_eks_cluster_in_us_east_2() {
    let secrets = FuncTestsSecrets::new();
    let region = "us-east-2".to_string();
    let aws_region = AwsRegion::from_str(&region).expect("Wasn't able to convert the desired region");
    create_and_destroy_eks_cluster(
        region,
        ClusterTestType::WithPause,
        AWS_KUBERNETES_MAJOR_VERSION,
        AWS_KUBERNETES_MINOR_VERSION,
        WithoutNatGateways,
        function_name!(),
    );
}

// only enable this test manually when we want to perform and validate upgrade process
#[cfg(feature = "test-aws-infra")]
#[named]
#[test]
#[ignore]
fn create_upgrade_and_destroy_eks_cluster_in_eu_west_3() {
    let region = "eu-west-3".to_string();
    create_and_destroy_eks_cluster(
        region,
        ClusterTestType::WithUpgrade,
        AWS_KUBERNETES_MAJOR_VERSION,
        AWS_KUBERNETES_MINOR_VERSION,
        WithoutNatGateways,
        function_name!(),
    );
}

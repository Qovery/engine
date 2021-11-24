extern crate test_utilities;

use self::test_utilities::aws::{AWS_KUBERNETES_MAJOR_VERSION, AWS_KUBERNETES_MINOR_VERSION};
use self::test_utilities::utilities::{engine_run_test, FuncTestsSecrets};
use ::function_name::named;
use qovery_engine::cloud_provider::aws::kubernetes::VpcQoveryNetworkMode;
use qovery_engine::cloud_provider::aws::kubernetes::VpcQoveryNetworkMode::{WithNatGateways, WithoutNatGateways};
use qovery_engine::cloud_provider::Kind;
use test_utilities::common::{cluster_test, ClusterTestType};

#[cfg(feature = "test-aws-infra")]
fn create_and_destroy_eks_cluster(
    region: &str,
    secrets: FuncTestsSecrets,
    test_type: ClusterTestType,
    major_boot_version: u8,
    minor_boot_version: u8,
    vpc_network_mode: VpcQoveryNetworkMode,
    test_name: &str,
) {
    engine_run_test(|| {
        cluster_test(
            test_name,
            Kind::Aws,
            region,
            secrets,
            test_type,
            major_boot_version,
            minor_boot_version,
            Option::from(vpc_network_mode),
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
    let region = "eu-west-3";
    let secrets = FuncTestsSecrets::new();
    create_and_destroy_eks_cluster(
        &region,
        secrets,
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
    let region = "eu-west-3";
    let secrets = FuncTestsSecrets::new();
    create_and_destroy_eks_cluster(
        &region,
        secrets,
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
    let region = "us-east-2";
    let secrets = FuncTestsSecrets::new();
    create_and_destroy_eks_cluster(
        &region,
        secrets,
        ClusterTestType::Classic,
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
    let region = "eu-west-3";
    let secrets = FuncTestsSecrets::new();

    create_and_destroy_eks_cluster(
        &region,
        secrets,
        ClusterTestType::WithUpgrade,
        AWS_KUBERNETES_MAJOR_VERSION,
        AWS_KUBERNETES_MINOR_VERSION,
        WithoutNatGateways,
        function_name!(),
    );
}

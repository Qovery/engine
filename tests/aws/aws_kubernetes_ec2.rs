extern crate test_utilities;

use self::test_utilities::utilities::{context, engine_run_test, generate_cluster_id, generate_id, logger};
use ::function_name::named;
use qovery_engine::cloud_provider::kubernetes::Kind as KKind;

use qovery_engine::cloud_provider::aws::kubernetes::VpcQoveryNetworkMode;
use qovery_engine::cloud_provider::aws::kubernetes::VpcQoveryNetworkMode::WithoutNatGateways;
use qovery_engine::cloud_provider::aws::regions::AwsRegion;
use qovery_engine::cloud_provider::Kind;
use std::str::FromStr;
use test_utilities::aws::{K3S_KUBERNETES_MAJOR_VERSION, K3S_KUBERNETES_MINOR_VERSION};
use test_utilities::common::{cluster_test, ClusterDomain, ClusterTestType};

#[cfg(feature = "test-aws-infra-ec2")]
fn create_and_destroy_aws_ec2_k3s_cluster(
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
        let cluster_id = generate_cluster_id(region.to_string().as_str());
        cluster_test(
            test_name,
            Kind::Aws,
            KKind::Ec2,
            context(generate_id().as_str(), cluster_id.as_str()),
            logger(),
            region.to_aws_format().as_str(),
            Some(zones),
            test_type,
            major_boot_version,
            minor_boot_version,
            &ClusterDomain::Default { cluster_id },
            Option::from(vpc_network_mode),
            None,
        )
    })
}

/*
    TESTS NOTES:
    It is useful to keep 2 clusters deployment tests to run in // to validate there is no name collision (overlaping)
*/

#[cfg(feature = "test-aws-infra-ec2")]
#[named]
#[test]
fn create_and_destroy_aws_ec2_k3s_cluster_eu_west_3() {
    let region = "eu-west-3".to_string();
    create_and_destroy_aws_ec2_k3s_cluster(
        region,
        ClusterTestType::Classic,
        K3S_KUBERNETES_MAJOR_VERSION,
        K3S_KUBERNETES_MINOR_VERSION,
        WithoutNatGateways,
        function_name!(),
    );
}

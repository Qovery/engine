use crate::helpers::utilities::{context_for_ec2, engine_run_test, generate_id, logger};
use ::function_name::named;
use qovery_engine::cloud_provider::kubernetes::Kind as KKind;

use crate::helpers::aws::{AWS_EC2_INSTANCE_TEST_REGION, K3S_KUBERNETES_MAJOR_VERSION, K3S_KUBERNETES_MINOR_VERSION};
use crate::helpers::common::ClusterDomain;
use crate::helpers::kubernetes::{cluster_test, ClusterTestType};
use crate::helpers::utilities::generate_cluster_id;
use qovery_engine::cloud_provider::aws::kubernetes::VpcQoveryNetworkMode;
use qovery_engine::cloud_provider::aws::kubernetes::VpcQoveryNetworkMode::WithoutNatGateways;
use qovery_engine::cloud_provider::Kind;
use qovery_engine::utilities::to_short_id;

fn create_and_destroy_aws_ec2_k3s_cluster(
    test_type: ClusterTestType,
    major_boot_version: u8,
    minor_boot_version: u8,
    vpc_network_mode: VpcQoveryNetworkMode,
    test_name: &str,
) {
    engine_run_test(|| -> String {
        let localisation = AWS_EC2_INSTANCE_TEST_REGION;
        let zones = localisation.get_zones();
        let cluster_id = generate_cluster_id(localisation.to_aws_format());
        cluster_test(
            test_name,
            Kind::Aws,
            KKind::Ec2,
            context_for_ec2(generate_id(), cluster_id),
            logger(),
            localisation.to_aws_format(),
            Some(zones),
            test_type,
            major_boot_version,
            minor_boot_version,
            &ClusterDomain::Default {
                cluster_id: to_short_id(&cluster_id),
            },
            Option::from(vpc_network_mode),
            None,
        )
    })
}

#[cfg(feature = "test-aws-ec2-infra")]
#[named]
#[test]
fn create_and_destroy_aws_ec2_k3s_cluster_us_east_2() {
    create_and_destroy_aws_ec2_k3s_cluster(
        ClusterTestType::Classic,
        K3S_KUBERNETES_MAJOR_VERSION,
        K3S_KUBERNETES_MINOR_VERSION,
        WithoutNatGateways,
        function_name!(),
    );
}

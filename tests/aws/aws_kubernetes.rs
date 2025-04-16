use std::str::FromStr;

use crate::helpers::common::{ActionableFeature, ClusterDomain, NodeManager};
use crate::helpers::utilities::{
    context_for_cluster, engine_run_test, generate_cluster_id, generate_organization_id, logger, metrics_registry,
};
use ::function_name::named;

use crate::helpers::kubernetes::{ClusterTestType, cluster_test};
use qovery_engine::environment::models::ToCloudProviderFormat;
use qovery_engine::infrastructure::models::cloud_provider::Kind;
use qovery_engine::infrastructure::models::cloud_provider::aws::regions::AwsRegion;
use qovery_engine::infrastructure::models::kubernetes::Kind as KKind;
use qovery_engine::infrastructure::models::kubernetes::aws::{
    KarpenterDefaultNodePoolOverride, KarpenterNodePool, KarpenterNodePoolDisruptionBudget,
    KarpenterNodePoolDisruptionReason, KarpenterNodePoolLimits, KarpenterNodePoolRequirement,
    KarpenterNodePoolRequirementKey, KarpenterParameters, KarpenterRequirementOperator,
    KarpenterStableNodePoolOverride,
};
use qovery_engine::io_models::models::VpcQoveryNetworkMode::{WithNatGateways, WithoutNatGateways};
use qovery_engine::io_models::models::{
    CpuArchitecture, KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit, VpcQoveryNetworkMode,
};
use qovery_engine::utilities::to_short_id;

#[cfg(any(
    feature = "test-aws-infra",
    feature = "test-aws-infra-arm",
    feature = "test-aws-infra-nat-gateway",
    feature = "test-aws-infra-upgrade",
    feature = "test-aws-infra-karpenter",
))]
fn create_and_destroy_eks_cluster(
    region: String,
    test_type: ClusterTestType,
    vpc_network_mode: VpcQoveryNetworkMode,
    test_name: &str,
    node_manager: NodeManager,
    actionable_features: Vec<ActionableFeature>,
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
            node_manager,
            actionable_features,
        )
    })
}

#[cfg(any(feature = "test-aws-infra-arm", feature = "test-aws-infra-upgrade"))]
fn create_and_destroy_arm64_eks_cluster(
    region: String,
    test_type: ClusterTestType,
    vpc_network_mode: VpcQoveryNetworkMode,
    test_name: &str,
    actionable_features: Vec<ActionableFeature>,
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
            NodeManager::Default,
            actionable_features,
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
    create_and_destroy_eks_cluster(
        region,
        ClusterTestType::Classic,
        WithoutNatGateways,
        function_name!(),
        NodeManager::Default,
        vec![],
    );
}

#[cfg(feature = "test-aws-infra-nat-gateway")]
#[named]
#[test]
fn create_and_destroy_eks_cluster_with_nat_gw_in_us_east_2() {
    let region = "us-east-2".to_string();
    create_and_destroy_eks_cluster(
        region,
        ClusterTestType::Classic,
        WithNatGateways,
        function_name!(),
        NodeManager::Default,
        vec![],
    );
}

#[cfg(feature = "test-aws-infra")]
#[ignore]
#[named]
#[test]
fn create_and_destroy_eks_cluster_in_us_east_2() {
    let region = "us-east-2".to_string();
    create_and_destroy_eks_cluster(
        region,
        ClusterTestType::Classic,
        WithoutNatGateways,
        function_name!(),
        NodeManager::Default,
        vec![],
    );
}

#[cfg(feature = "test-aws-infra")]
#[named]
#[test]
fn create_pause_and_destroy_eks_cluster_in_us_east_2() {
    let region = "us-east-2".to_string();
    create_and_destroy_eks_cluster(
        region,
        ClusterTestType::WithPause,
        WithoutNatGateways,
        function_name!(),
        NodeManager::Default,
        vec![],
    );
}

#[cfg(feature = "test-aws-infra")]
#[named]
#[test]
fn create_destroy_eks_cluster_with_metrics_in_us_east_2() {
    let region = "us-east-2".to_string();
    create_and_destroy_eks_cluster(
        region,
        ClusterTestType::Classic,
        WithoutNatGateways,
        function_name!(),
        NodeManager::Default,
        vec![ActionableFeature::Metrics],
    );
}

// only enable this test manually when we want to perform and validate upgrade process
#[cfg(feature = "test-aws-infra-upgrade")]
#[named]
#[test]
fn create_upgrade_and_destroy_eks_cluster_in_eu_west_3() {
    let region = "eu-west-3".to_string();
    create_and_destroy_eks_cluster(
        region,
        ClusterTestType::WithUpgrade,
        WithoutNatGateways,
        function_name!(),
        NodeManager::Default,
        vec![],
    );
}

// ARM64

#[cfg(feature = "test-aws-infra-arm")]
#[named]
#[test]
fn create_and_destroy_eks_cluster_arm64_without_nat_gw_in_eu_west_3() {
    let region = "eu-west-3".to_string();
    create_and_destroy_arm64_eks_cluster(
        region,
        ClusterTestType::Classic,
        WithoutNatGateways,
        function_name!(),
        vec![],
    );
}

#[cfg(feature = "test-aws-infra-upgrade")]
#[named]
#[test]
fn create_upgrade_and_destroy_eks_cluster_arm64_without_nat_gw_in_eu_west_3() {
    let region = "eu-west-3".to_string();
    create_and_destroy_arm64_eks_cluster(
        region,
        ClusterTestType::WithUpgrade,
        WithoutNatGateways,
        function_name!(),
        vec![],
    );
}

// Karpenter

#[cfg(feature = "test-aws-infra-karpenter")]
#[named]
#[test]
fn create_and_destroy_eks_cluster_karpenter_without_nat_gw_in_eu_west_3() {
    let region = "eu-west-3".to_string();
    let karpenter_parameters = KarpenterParameters {
        spot_enabled: true,
        max_node_drain_time_in_secs: None,
        disk_size_in_gib: 50,
        default_service_architecture: CpuArchitecture::AMD64,
        qovery_node_pools: KarpenterNodePool {
            requirements: vec![
                KarpenterNodePoolRequirement {
                    key: KarpenterNodePoolRequirementKey::InstanceFamily,
                    values: vec!["t2".to_string(), "t3".to_string(), "t3a".to_string()],
                    operator: Some(KarpenterRequirementOperator::In),
                },
                KarpenterNodePoolRequirement {
                    key: KarpenterNodePoolRequirementKey::InstanceSize,
                    values: vec!["large".to_string()],
                    operator: Some(KarpenterRequirementOperator::In),
                },
                KarpenterNodePoolRequirement {
                    key: KarpenterNodePoolRequirementKey::Arch,
                    values: vec!["AMD64".to_string()],
                    operator: Some(KarpenterRequirementOperator::In),
                },
            ],
            stable_override: KarpenterStableNodePoolOverride {
                budgets: vec![KarpenterNodePoolDisruptionBudget {
                    nodes: "0".to_string(),
                    reasons: vec![KarpenterNodePoolDisruptionReason::Underutilized],
                    duration: duration_str::parse("24h").unwrap(),
                    schedule: "0 0 * * *".to_string(),
                }],
                limits: Some(KarpenterNodePoolLimits {
                    max_cpu: KubernetesCpuResourceUnit::MilliCpu(10_000),
                    max_memory: KubernetesMemoryResourceUnit::GibiByte(20),
                }),
            },
            default_override: Some(KarpenterDefaultNodePoolOverride {
                limits: Some(KarpenterNodePoolLimits {
                    max_cpu: KubernetesCpuResourceUnit::MilliCpu(10_000),
                    max_memory: KubernetesMemoryResourceUnit::GibiByte(20),
                }),
            }),
        },
    };
    create_and_destroy_eks_cluster(
        region,
        ClusterTestType::Classic,
        WithoutNatGateways,
        function_name!(),
        NodeManager::Karpenter {
            config: karpenter_parameters,
        },
        vec![],
    );
}

#[cfg(feature = "test-aws-infra-karpenter")]
#[named]
#[test]
fn create_pause_and_destroy_eks_cluster_arm_karpenter_with_nat_gw_in_eu_west_3() {
    let region = "eu-west-3".to_string();
    let karpenter_parameters = KarpenterParameters {
        spot_enabled: true,
        max_node_drain_time_in_secs: None,
        disk_size_in_gib: 50,
        default_service_architecture: CpuArchitecture::ARM64,
        qovery_node_pools: KarpenterNodePool {
            requirements: vec![
                KarpenterNodePoolRequirement {
                    key: KarpenterNodePoolRequirementKey::InstanceFamily,
                    values: vec!["c6g".to_string(), "c7g".to_string(), "t4g".to_string()],
                    operator: Some(KarpenterRequirementOperator::In),
                },
                KarpenterNodePoolRequirement {
                    key: KarpenterNodePoolRequirementKey::Arch,
                    values: vec!["ARM64".to_string()],
                    operator: Some(KarpenterRequirementOperator::In),
                },
            ],
            stable_override: KarpenterStableNodePoolOverride {
                budgets: vec![KarpenterNodePoolDisruptionBudget {
                    nodes: "0".to_string(),
                    reasons: vec![KarpenterNodePoolDisruptionReason::Underutilized],
                    duration: duration_str::parse("24h").unwrap(),
                    schedule: "0 0 * * *".to_string(),
                }],
                limits: Some(KarpenterNodePoolLimits {
                    max_cpu: KubernetesCpuResourceUnit::MilliCpu(10_000),
                    max_memory: KubernetesMemoryResourceUnit::GibiByte(20),
                }),
            },
            default_override: Some(KarpenterDefaultNodePoolOverride {
                limits: Some(KarpenterNodePoolLimits {
                    max_cpu: KubernetesCpuResourceUnit::MilliCpu(10_000),
                    max_memory: KubernetesMemoryResourceUnit::GibiByte(20),
                }),
            }),
        },
    };
    create_and_destroy_eks_cluster(
        region,
        ClusterTestType::WithPause,
        WithNatGateways,
        function_name!(),
        NodeManager::Karpenter {
            config: karpenter_parameters,
        },
        vec![],
    );
}

// only enable this test manually when we want to perform and validate upgrade process
#[cfg(feature = "test-aws-infra-upgrade")]
#[test]
#[named]
fn create_upgrade_and_destroy_eks_cluster_karpenter_with_nat_gw_in_eu_west_3() {
    let region = "eu-west-3".to_string();
    let karpenter_parameters = KarpenterParameters {
        spot_enabled: false,
        max_node_drain_time_in_secs: None,
        disk_size_in_gib: 50,
        default_service_architecture: CpuArchitecture::AMD64,
        qovery_node_pools: KarpenterNodePool {
            requirements: vec![
                KarpenterNodePoolRequirement {
                    key: KarpenterNodePoolRequirementKey::InstanceFamily,
                    values: vec!["t2".to_string(), "t3".to_string(), "t3a".to_string()],
                    operator: Some(KarpenterRequirementOperator::In),
                },
                KarpenterNodePoolRequirement {
                    key: KarpenterNodePoolRequirementKey::InstanceSize,
                    values: vec!["large".to_string()],
                    operator: Some(KarpenterRequirementOperator::In),
                },
                KarpenterNodePoolRequirement {
                    key: KarpenterNodePoolRequirementKey::Arch,
                    values: vec!["AMD64".to_string()],
                    operator: Some(KarpenterRequirementOperator::In),
                },
            ],
            stable_override: KarpenterStableNodePoolOverride {
                budgets: vec![KarpenterNodePoolDisruptionBudget {
                    nodes: "0".to_string(),
                    reasons: vec![KarpenterNodePoolDisruptionReason::Underutilized],
                    duration: duration_str::parse("24h").unwrap(),
                    schedule: "0 0 * * *".to_string(),
                }],
                limits: None,
            },
            default_override: None,
        },
    };
    create_and_destroy_eks_cluster(
        region,
        ClusterTestType::WithUpgrade,
        WithoutNatGateways,
        function_name!(),
        NodeManager::Karpenter {
            config: karpenter_parameters,
        },
        vec![],
    );
}

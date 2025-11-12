use crate::helpers;
use crate::helpers::common::{ClusterDomain, NodeManager};
use crate::helpers::kubernetes::{ClusterTestType, cluster_test};
use crate::helpers::utilities::{
    FuncTestsSecrets, context_for_cluster, engine_run_test, generate_cluster_id, generate_id, logger, metrics_registry,
};
use ::function_name::named;
use qovery_engine::environment::models::ToCloudProviderFormat;
use qovery_engine::infrastructure::models::cloud_provider::Kind;
use qovery_engine::infrastructure::models::cloud_provider::aws::regions::AwsRegion;
use qovery_engine::infrastructure::models::disk_size::DiskSize;
use qovery_engine::infrastructure::models::kubernetes::Kind as KKind;
use qovery_engine::infrastructure::models::kubernetes::karpenter::{
    KarpenterDefaultNodePoolOverride, KarpenterGpuNodePoolOverride, KarpenterNodePool,
    KarpenterNodePoolDisruptionBudget, KarpenterNodePoolDisruptionReason, KarpenterNodePoolLimits,
    KarpenterNodePoolRequirement, KarpenterNodePoolRequirementKey, KarpenterParameters, KarpenterRequirementOperator,
    KarpenterStableNodePoolOverride,
};
use qovery_engine::io_models::models::VpcQoveryNetworkMode::WithNatGateways;
use qovery_engine::io_models::models::{CpuArchitecture, KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};
use qovery_engine::utilities::to_short_id;
use std::str::FromStr;

#[cfg(feature = "test-aws-whole-enchilada")]
#[named]
#[test]
fn create_and_destroy_eks_cluster_with_env_in_eu_west_3() {
    let secrets = FuncTestsSecrets::new();

    let region = secrets
        .AWS_DEFAULT_REGION
        .as_ref()
        .expect("AWS region was not found in secrets");
    let aws_region = AwsRegion::from_str(region).expect("Wasn't able to parse the desired region");
    let aws_zones = aws_region.zones();

    let organization_id = generate_id();
    let cluster_id = generate_cluster_id(aws_region.to_string().as_str());
    let context = context_for_cluster(organization_id, cluster_id, Some(KKind::Eks));

    let cluster_domain = format!(
        "{}.{}",
        to_short_id(&cluster_id),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str()
    );

    let environment = helpers::environment::working_minimal_environment(&context);
    let env_action = environment;

    engine_run_test(|| {
        cluster_test(
            function_name!(),
            Kind::Aws,
            KKind::Eks,
            context.clone(),
            logger(),
            metrics_registry(),
            region,
            Some(aws_zones.iter().map(|z| z.to_cloud_provider_format()).collect()),
            ClusterTestType::Classic,
            &ClusterDomain::Custom { domain: cluster_domain },
            Some(WithNatGateways),
            CpuArchitecture::AMD64,
            Some(&env_action),
            NodeManager::Default,
            vec![],
        )
    })
}

#[cfg(feature = "test-aws-whole-enchilada")]
#[named]
#[test]
fn create_resize_and_destroy_eks_cluster_with_env_in_eu_west_3() {
    let secrets = FuncTestsSecrets::new();

    let region = secrets
        .AWS_DEFAULT_REGION
        .as_ref()
        .expect("AWS region was not found in secrets");
    let aws_region = AwsRegion::from_str(region).expect("Wasn't able to convert the desired region");
    let aws_zones = aws_region.zones();

    let organization_id = generate_id();
    let cluster_id = generate_cluster_id(aws_region.to_string().as_str());
    let context = context_for_cluster(organization_id, cluster_id, Some(KKind::Eks));

    let cluster_domain = format!(
        "{}.{}",
        to_short_id(&cluster_id),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str()
    );

    engine_run_test(|| {
        cluster_test(
            function_name!(),
            Kind::Aws,
            KKind::Eks,
            context.clone(),
            logger(),
            metrics_registry(),
            region,
            Some(aws_zones.iter().map(|z| z.to_cloud_provider_format()).collect()),
            ClusterTestType::WithNodesResize,
            &ClusterDomain::Custom { domain: cluster_domain },
            None,
            CpuArchitecture::AMD64,
            None,
            NodeManager::Default,
            vec![],
        )
    })
}

#[cfg(feature = "test-aws-whole-enchilada")]
#[ignore]
#[named]
#[test]
fn create_pause_and_destroy_eks_cluster_with_env_in_eu_west_3() {
    let secrets = FuncTestsSecrets::new();

    let region = secrets.AWS_DEFAULT_REGION.as_ref().expect("AWS region was not found");
    let aws_region = AwsRegion::from_str(region).expect("Wasn't able to parse the desired region");
    let aws_zones = aws_region.zones();

    let organization_id = generate_id();
    let cluster_id = generate_cluster_id(aws_region.to_string().as_str());
    let context = context_for_cluster(organization_id, cluster_id, Some(KKind::Eks));

    let cluster_domain = format!(
        "{}.{}",
        to_short_id(&cluster_id),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str()
    );

    let environment = helpers::environment::working_minimal_environment(&context);
    let env_action = environment;

    engine_run_test(|| {
        cluster_test(
            function_name!(),
            Kind::Aws,
            KKind::Eks,
            context.clone(),
            logger(),
            metrics_registry(),
            region,
            Some(aws_zones.iter().map(|z| z.to_cloud_provider_format()).collect()),
            ClusterTestType::WithPause,
            &ClusterDomain::Custom { domain: cluster_domain },
            Some(WithNatGateways),
            CpuArchitecture::AMD64,
            Some(&env_action),
            NodeManager::Default,
            vec![],
        )
    })
}

#[cfg(feature = "test-aws-whole-enchilada")]
#[ignore]
#[named]
#[test]
fn create_upgrade_and_destroy_eks_cluster_with_env_in_eu_west_3() {
    let secrets = FuncTestsSecrets::new();

    let region = secrets.AWS_DEFAULT_REGION.as_ref().expect("AWS region was not found");
    let aws_region = AwsRegion::from_str(region).expect("Wasn't able to parse the desired region");
    let aws_zones = aws_region.zones();

    let organization_id = generate_id();
    let cluster_id = generate_cluster_id(aws_region.to_string().as_str());
    let context = context_for_cluster(organization_id, cluster_id, Some(KKind::Eks));

    let cluster_domain = format!(
        "{}.{}",
        to_short_id(&cluster_id),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str()
    );

    let environment = helpers::environment::working_minimal_environment(&context);
    let env_action = environment;

    engine_run_test(|| {
        cluster_test(
            function_name!(),
            Kind::Aws,
            KKind::Eks,
            context.clone(),
            logger(),
            metrics_registry(),
            region,
            Some(aws_zones.iter().map(|z| z.to_cloud_provider_format()).collect()),
            ClusterTestType::WithUpgrade,
            &ClusterDomain::Custom { domain: cluster_domain },
            Some(WithNatGateways),
            CpuArchitecture::AMD64,
            Some(&env_action),
            NodeManager::Default,
            vec![],
        )
    })
}

#[cfg(feature = "test-aws-whole-enchilada-gpu")]
#[ignore]
#[named]
#[test]
fn create_upgrade_and_destroy_eks_cluster_gpu_with_env_in_eu_west_3() {
    let secrets = FuncTestsSecrets::new();

    let region = secrets.AWS_DEFAULT_REGION.as_ref().expect("AWS region was not found");
    let aws_region = AwsRegion::from_str(region).expect("Wasn't able to parse the desired region");
    let aws_zones = aws_region.zones();

    let organization_id = generate_id();
    let cluster_id = generate_cluster_id(aws_region.to_string().as_str());
    let context = context_for_cluster(organization_id, cluster_id, Some(KKind::Eks));

    let cluster_domain = format!(
        "{}.{}",
        to_short_id(&cluster_id),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str()
    );

    let environment = helpers::environment::working_minimal_environment(&context);
    let env_action = environment;

    engine_run_test(|| {
        cluster_test(
            function_name!(),
            Kind::Aws,
            KKind::Eks,
            context.clone(),
            logger(),
            metrics_registry(),
            region,
            Some(aws_zones.iter().map(|z| z.to_cloud_provider_format()).collect()),
            ClusterTestType::WithUpgrade,
            &ClusterDomain::Custom { domain: cluster_domain },
            Some(WithNatGateways),
            CpuArchitecture::AMD64,
            Some(&env_action),
            NodeManager::Karpenter {
                config: KarpenterParameters {
                    spot_enabled: true,
                    max_node_drain_time_in_secs: None,
                    disk_size: DiskSize::Gib(50),
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
                        gpu_override: Some(KarpenterGpuNodePoolOverride {
                            spot_enabled: true,
                            disk_size: DiskSize::Gib(100),
                            requirements: Some(vec![
                                KarpenterNodePoolRequirement {
                                    key: KarpenterNodePoolRequirementKey::InstanceFamily,
                                    values: vec!["g4dn".to_string(), "g5".to_string()],
                                    operator: Some(KarpenterRequirementOperator::In),
                                },
                                KarpenterNodePoolRequirement {
                                    key: KarpenterNodePoolRequirementKey::InstanceSize,
                                    values: vec!["xlarge".to_string(), "2xlarge".to_string()],
                                    operator: Some(KarpenterRequirementOperator::In),
                                },
                                KarpenterNodePoolRequirement {
                                    key: KarpenterNodePoolRequirementKey::Arch,
                                    values: vec!["AMD64".to_string()],
                                    operator: Some(KarpenterRequirementOperator::In),
                                },
                            ]),
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
                        }),
                        default_override: Some(KarpenterDefaultNodePoolOverride {
                            limits: Some(KarpenterNodePoolLimits {
                                max_cpu: KubernetesCpuResourceUnit::MilliCpu(10_000),
                                max_memory: KubernetesMemoryResourceUnit::GibiByte(20),
                            }),
                        }),
                    },
                },
            },
            vec![],
        )
    })
}

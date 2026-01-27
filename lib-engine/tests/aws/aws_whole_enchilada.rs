use crate::helpers;
use crate::helpers::aws::{AWS_KUBERNETES_VERSION, AWS_RESOURCE_TTL_IN_SECONDS, container_registry_ecr};
use crate::helpers::common::{Cluster, ClusterDomain, NodeManager};
use crate::helpers::dns::dns_provider_qoverydns;
use crate::helpers::kubernetes::{ClusterTestType, cluster_test};
use crate::helpers::utilities::{
    FuncTestsSecrets, build_platform_local_docker, context_for_cluster, engine_run_test, generate_cluster_id,
    generate_id, logger, metrics_registry,
};
use ::function_name::named;
use chrono::Utc;
use qovery_engine::environment::models::ToCloudProviderFormat;
use qovery_engine::environment::models::types::Percentage;
use qovery_engine::environment::task::EnvironmentTask;
use qovery_engine::fs::workspace_directory;
use qovery_engine::infrastructure::infrastructure_context::InfrastructureContext;
use qovery_engine::infrastructure::models::cloud_provider::aws::AWS;
use qovery_engine::infrastructure::models::cloud_provider::aws::regions::AwsRegion;
use qovery_engine::infrastructure::models::cloud_provider::io::ClusterAdvancedSettings;
use qovery_engine::infrastructure::models::cloud_provider::{CloudProvider, Kind};
use qovery_engine::infrastructure::models::container_registry::ContainerRegistry;
use qovery_engine::infrastructure::models::disk_size::DiskSize;
use qovery_engine::infrastructure::models::dns_provider::DnsProvider;
use qovery_engine::infrastructure::models::kubernetes::Kind as KKind;
use qovery_engine::infrastructure::models::kubernetes::aws::AwsStorageType;
use qovery_engine::infrastructure::models::kubernetes::aws::eks::EKS;
use qovery_engine::infrastructure::models::kubernetes::karpenter::{
    KarpenterDefaultNodePoolOverride, KarpenterGpuNodePoolOverride, KarpenterNodePool,
    KarpenterNodePoolDisruptionBudget, KarpenterNodePoolDisruptionReason, KarpenterNodePoolLimits,
    KarpenterNodePoolRequirement, KarpenterNodePoolRequirementKey, KarpenterParameters, KarpenterRequirementOperator,
    KarpenterStableNodePoolOverride,
};
use qovery_engine::io_models::QoveryIdentifier;
use qovery_engine::io_models::engine_location::EngineLocation;
use qovery_engine::io_models::models::StorageClass;
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
                    disk_iops: None,
                    disk_throughput: None,
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
                            disk_iops: None,
                            disk_throughput: None,
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

#[cfg(feature = "test-aws-whole-enchilada")]
#[named]
#[test]
fn create_and_destroy_eks_cluster_with_env_and_gateway_api_in_eu_west_3() {
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
#[ignore]
#[named]
#[test]
fn create_and_destroy_eks_cluster_with_all_gateway_api_envoy_settings_in_eu_west_3() {
    let secrets = FuncTestsSecrets::new();

    let region = secrets
        .AWS_DEFAULT_REGION
        .as_ref()
        .expect("AWS region was not found in secrets");
    let aws_region = AwsRegion::from_str(region).expect("Wasn't able to parse the desired region");

    let organization_id = generate_id();
    let cluster_id = generate_cluster_id(aws_region.to_string().as_str());
    let context = context_for_cluster(organization_id, cluster_id, Some(KKind::Eks));

    let cluster_domain_str = format!(
        "{}.{}",
        to_short_id(&cluster_id),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str()
    );

    let environment = helpers::environment::working_minimal_environment(&context);

    engine_run_test(|| {
        let log = logger();
        let metrics = metrics_registry();
        let cluster_domain = ClusterDomain::Custom {
            domain: cluster_domain_str.clone(),
        };

        // Build infrastructure components
        let container_registry = ContainerRegistry::Ecr(container_registry_ecr(&context, log.clone()));
        let build_platform = Box::new(build_platform_local_docker(&context));
        let cloud_provider: Box<dyn CloudProvider> = AWS::cloud_provider(&context, KKind::Eks, region);
        let dns_provider: Box<dyn DnsProvider> = dns_provider_qoverydns(&context, &cluster_domain);

        // Create temp directory for cluster
        let temp_dir = workspace_directory(
            context.workspace_root_dir(),
            context.execution_id(),
            format!("bootstrap/{}", context.cluster_short_id()),
        )
        .unwrap();

        // Get AWS options
        let options = AWS::kubernetes_cluster_options(
            secrets.clone(),
            QoveryIdentifier::new(*context.cluster_long_id()),
            EngineLocation::ClientSide,
            Some(WithNatGateways),
        );

        // Create custom advanced settings with all Envoy and Gateway API settings enabled
        let advanced_settings = ClusterAdvancedSettings {
            pleco_resources_ttl: AWS_RESOURCE_TTL_IN_SECONDS as i32,
            aws_vpc_enable_flow_logs: true,
            aws_eks_ec2_metadata_imds: qovery_engine::infrastructure::models::cloud_provider::io::AwsEc2MetadataImds::Required,
            aws_eks_enable_alb_controller: true,
            k8s_storage_class_fast_ssd: qovery_engine::infrastructure::models::cloud_provider::io::StorageClass::from(
                StorageClass(AwsStorageType::GP2.to_k8s_storage_class()),
            ),
            // Gateway API settings
            k8s_deploy_api_gateway: Some(true),
            k8s_use_api_gateway: Some(true),
            // Envoy HPA settings
            envoy_hpa_cpu_average_utilization_percentage_threshold: Some(Percentage::try_from(0.75).unwrap()),
            envoy_hpa_memory_average_utilization_percentage_threshold: Some(Percentage::try_from(0.80).unwrap()),
            envoy_hpa_min_number_instances: 3,
            envoy_hpa_max_number_instances: 50,
            // Envoy resource settings
            envoy_vcpu_request_in_milli_cpu: 250,
            envoy_vcpu_limit_in_milli_cpu: 2000,
            envoy_memory_request_in_mib: 512,
            envoy_memory_limit_in_mib: 2048,
            // Envoy client IP detection
            envoy_client_ip_detection_x_forwarded_for_number_trusted_hops: Some(2),
            // Envoy access log format (JSON format)
            envoy_access_log_format: Some(
                r#"{"time":"%START_TIME%","method":"%REQ(:METHOD)%","path":"%REQ(X-ENVOY-ORIGINAL-PATH?:PATH)%","protocol":"%PROTOCOL%","response_code":"%RESPONSE_CODE%","response_flags":"%RESPONSE_FLAGS%","bytes_received":"%BYTES_RECEIVED%","bytes_sent":"%BYTES_SENT%","duration":"%DURATION%","upstream_service_time":"%RESP(X-ENVOY-UPSTREAM-SERVICE-TIME)%","x_forwarded_for":"%REQ(X-FORWARDED-FOR)%","user_agent":"%REQ(USER-AGENT)%","request_id":"%REQ(X-REQUEST-ID)%","authority":"%REQ(:AUTHORITY)%","upstream_host":"%UPSTREAM_HOST%"}"#.to_string()
            ),
            // Envoy custom HTTP errors
            envoy_custom_http_errors_default: Some(vec![404, 503, 500, 502]),
            // Envoy compression
            envoy_enable_compression: true,
            // Envoy default backend
            // envoy_default_backend_enable: true,
            // envoy_default_backend_image: Some("ghcr.io/qovery/http-errors-server".to_string()),
            // envoy_default_backend_tag: Some("latest".to_string()),
            ..Default::default()
        };

        // Create EKS cluster with custom advanced settings
        let kubernetes = Box::new(
            EKS::new(
                context.clone(),
                *context.cluster_long_id(),
                format!("qovery-{}", context.cluster_short_id()).as_str(),
                AWS_KUBERNETES_VERSION,
                aws_region.clone(),
                aws_region.get_zones_to_string(),
                cloud_provider.as_ref(),
                Utc::now(),
                options,
                AWS::kubernetes_nodes(3, 10, CpuArchitecture::AMD64),
                log.clone(),
                advanced_settings,
                None,
                None,
                temp_dir,
                None,
            )
            .unwrap(),
        );

        // Create infrastructure context
        let infra_ctx = InfrastructureContext::new(
            context.clone(),
            build_platform,
            container_registry,
            cloud_provider,
            dns_provider,
            kubernetes,
            metrics,
            true,
        );

        // Create cluster
        let create_result = infra_ctx
            .kubernetes()
            .as_infra_actions()
            .create_cluster(&infra_ctx, false);
        assert!(create_result.is_ok(), "Cluster creation should succeed");

        // Update cluster (second pass)
        let update_result = infra_ctx
            .kubernetes()
            .as_infra_actions()
            .create_cluster(&infra_ctx, false);
        assert!(update_result.is_ok(), "Cluster update should succeed");

        // Deploy environment
        let mut env = environment
            .to_environment_domain(
                &context,
                infra_ctx.cloud_provider(),
                infra_ctx.container_registry(),
                infra_ctx.kubernetes(),
            )
            .unwrap();

        env.action = qovery_engine::infrastructure::models::cloud_provider::service::Action::Create;
        EnvironmentTask::deploy_environment(env, &infra_ctx, &|| {
            qovery_engine::environment::models::abort::AbortStatus::None
        })
        .expect("Environment deployment should succeed");

        // Recreate environment for deletion
        let mut env_delete = environment
            .to_environment_domain(
                &context,
                infra_ctx.cloud_provider(),
                infra_ctx.container_registry(),
                infra_ctx.kubernetes(),
            )
            .unwrap();
        env_delete.action = qovery_engine::infrastructure::models::cloud_provider::service::Action::Delete;
        EnvironmentTask::deploy_environment(env_delete, &infra_ctx, &|| {
            qovery_engine::environment::models::abort::AbortStatus::None
        })
        .expect("Environment deletion should succeed");

        // Delete cluster
        let delete_result = infra_ctx.kubernetes().as_infra_actions().delete_cluster(&infra_ctx);
        assert!(delete_result.is_ok(), "Cluster deletion should succeed");

        function_name!().to_string()
    })
}

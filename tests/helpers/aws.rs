extern crate serde;
extern crate serde_derive;

use crate::helpers::common::{ActionableFeature, Cluster, ClusterDomain, NodeManager};
use crate::helpers::dns::dns_provider_qoverydns;
use crate::helpers::kubernetes::{
    KUBERNETES_MAX_NODES, KUBERNETES_MIN_NODES, TargetCluster, get_environment_test_kubernetes,
};
use crate::helpers::utilities::{FuncTestsSecrets, build_platform_local_docker};
use qovery_engine::environment::models::ToCloudProviderFormat;
use qovery_engine::environment::models::aws::AwsStorageType;
use qovery_engine::infrastructure::infrastructure_context::InfrastructureContext;
use qovery_engine::infrastructure::models::cloud_provider::aws::database_instance_type::AwsDatabaseInstanceType;
use qovery_engine::infrastructure::models::cloud_provider::aws::regions::AwsRegion;
use qovery_engine::infrastructure::models::cloud_provider::aws::{AWS, AwsCredentials};
use qovery_engine::infrastructure::models::cloud_provider::{CloudProvider, TerraformStateCredentials};
use qovery_engine::infrastructure::models::container_registry::ContainerRegistry;
use qovery_engine::infrastructure::models::container_registry::ecr::ECR;
use qovery_engine::infrastructure::models::dns_provider::DnsProvider;
use qovery_engine::infrastructure::models::kubernetes::aws::Options;
use qovery_engine::infrastructure::models::kubernetes::{Kind as KubernetesKind, KubernetesVersion};
use qovery_engine::io_models::context::Context;
use qovery_engine::io_models::engine_location::EngineLocation;
use qovery_engine::io_models::models::{CpuArchitecture, NodeGroups, StorageClass, VpcQoveryNetworkMode};
use qovery_engine::logger::Logger;
use qovery_engine::metrics_registry::MetricsRegistry;
use std::str::FromStr;
use tracing::error;
use uuid::Uuid;

pub const AWS_REGION_FOR_S3: AwsRegion = AwsRegion::EuWest3;
pub const AWS_KUBERNETES_VERSION: KubernetesVersion = KubernetesVersion::V1_31 {
    prefix: None,
    patch: None,
    suffix: None,
};
pub const AWS_DATABASE_INSTANCE_TYPE: AwsDatabaseInstanceType = AwsDatabaseInstanceType::DB_T3_MICRO;
pub const AWS_RESOURCE_TTL_IN_SECONDS: u32 = 9000;
pub const AWS_QUICK_RESOURCE_TTL_IN_SECONDS: u32 = 3600;

pub fn container_registry_ecr(context: &Context, logger: Box<dyn Logger>) -> ECR {
    let secrets = FuncTestsSecrets::new();
    if secrets.AWS_ACCESS_KEY_ID.is_none()
        || secrets.AWS_SECRET_ACCESS_KEY.is_none()
        || secrets.AWS_DEFAULT_REGION.is_none()
    {
        error!(
            "Please check your Vault connectivity (token/address) or AWS_ACCESS_KEY_ID/AWS_SECRET_ACCESS_KEY/AWS_DEFAULT_REGION envrionment variables are set"
        );
        std::process::exit(1)
    }

    let credentials = AwsCredentials::new(
        secrets.AWS_ACCESS_KEY_ID.expect("Unable to get access key"),
        secrets.AWS_SECRET_ACCESS_KEY.expect("Unable to get secret key"),
        secrets.AWS_SESSION_TOKEN,
    );

    let region =
        rusoto_core::Region::from_str(&secrets.AWS_DEFAULT_REGION.expect("Unable to get default region")).unwrap();
    ECR::new(
        context.clone(),
        Uuid::new_v4(),
        "ea59qe62xaw3wjai",
        credentials,
        region,
        logger,
        hashmap! {},
    )
    .unwrap()
}

/// This method is dedicated to test services deployments
/// `node_manager` is set to default (no Karpenter)
/// `actionable_features` is empty
pub fn aws_infra_config(
    targeted_cluster: &TargetCluster,
    context: &Context,
    logger: Box<dyn Logger>,
    metrics_registry: Box<dyn MetricsRegistry>,
) -> InfrastructureContext {
    let secrets = FuncTestsSecrets::new();

    AWS::docker_cr_engine(
        context,
        logger,
        metrics_registry,
        secrets
            .AWS_TEST_CLUSTER_REGION
            .expect("AWS_TEST_CLUSTER_REGION is not set")
            .as_str(),
        KubernetesKind::Eks,
        AWS_KUBERNETES_VERSION,
        &ClusterDomain::Default {
            cluster_id: context.cluster_short_id().to_string(),
        },
        None,
        KUBERNETES_MIN_NODES,
        KUBERNETES_MAX_NODES,
        CpuArchitecture::AMD64,
        EngineLocation::ClientSide,
        match targeted_cluster {
            TargetCluster::MutualizedTestCluster { kubeconfig } => Some(kubeconfig.to_string()), // <- using test cluster, not creating a new one
            TargetCluster::New => None, // <- creating a new cluster
        },
        NodeManager::Default,
        vec![],
    )
}

impl Cluster<AWS, Options> for AWS {
    fn docker_cr_engine(
        context: &Context,
        logger: Box<dyn Logger>,
        metrics_registry: Box<dyn MetricsRegistry>,
        region: &str,
        kubernetes_kind: KubernetesKind,
        kubernetes_version: KubernetesVersion,
        cluster_domain: &ClusterDomain,
        vpc_network_mode: Option<VpcQoveryNetworkMode>,
        min_nodes: i32,
        max_nodes: i32,
        cpu_archi: CpuArchitecture,
        engine_location: EngineLocation,
        kubeconfig: Option<String>,
        node_manager: NodeManager,
        actionable_features: Vec<ActionableFeature>,
    ) -> InfrastructureContext {
        // use ECR
        let container_registry = ContainerRegistry::Ecr(container_registry_ecr(context, logger.clone()));

        // use LocalDocker
        let build_platform = Box::new(build_platform_local_docker(context));

        // use AWS
        let cloud_provider: Box<dyn CloudProvider> = AWS::cloud_provider(context, kubernetes_kind, region);
        let dns_provider: Box<dyn DnsProvider> = dns_provider_qoverydns(context, cluster_domain);

        let kubernetes = get_environment_test_kubernetes(
            context,
            cloud_provider.as_ref(),
            kubernetes_version,
            logger.clone(),
            region,
            vpc_network_mode,
            min_nodes,
            max_nodes,
            cpu_archi,
            engine_location,
            StorageClass(AwsStorageType::GP2.to_k8s_storage_class()),
            kubeconfig,
            node_manager,
            actionable_features,
        );

        InfrastructureContext::new(
            context.clone(),
            build_platform,
            container_registry,
            cloud_provider,
            dns_provider,
            kubernetes,
            metrics_registry,
            true,
        )
    }

    fn cloud_provider(_context: &Context, kubernetes_kind: KubernetesKind, localisation: &str) -> Box<AWS> {
        let secrets = FuncTestsSecrets::new();
        let aws_region = match localisation {
            "EuWest3" => {
                AwsRegion::from_str(secrets.AWS_DEFAULT_REGION.unwrap().as_str()).expect("AWS region not supported")
            }
            "eu-west-3" => {
                AwsRegion::from_str(secrets.AWS_DEFAULT_REGION.unwrap().as_str()).expect("AWS region not supported")
            }
            "us-east-2" => AwsRegion::UsEast2,
            _ => panic!("Invalid cluster localisation {localisation}"),
        };

        let credentials = AwsCredentials::new(
            secrets.AWS_ACCESS_KEY_ID.expect("AWS_ACCESS_KEY_ID is not set"),
            secrets.AWS_SECRET_ACCESS_KEY.expect("AWS_SECRET_ACCESS_KEY is not set"),
            secrets.AWS_SESSION_TOKEN,
        );
        Box::new(AWS::new(
            Uuid::new_v4(),
            credentials,
            aws_region.to_cloud_provider_format(),
            aws_region.get_zones_to_string(),
            kubernetes_kind,
            TerraformStateCredentials {
                access_key_id: secrets
                    .TERRAFORM_AWS_ACCESS_KEY_ID
                    .expect("TERRAFORM_AWS_ACCESS_KEY_ID is n ot set"),
                secret_access_key: secrets
                    .TERRAFORM_AWS_SECRET_ACCESS_KEY
                    .expect("TERRAFORM_AWS_SECRET_ACCESS_KEY is not set"),
                region: secrets.TERRAFORM_AWS_REGION.expect("TERRAFORM_AWS_REGION is not set"),
                s3_bucket: secrets.TERRAFORM_AWS_BUCKET.expect("TERRAFORM_AWS_BUCKET is not set"),
                dynamodb_table: secrets
                    .TERRAFORM_AWS_DYNAMODB_TABLE
                    .expect("TERRAFORM_AWS_DYNAMODB_TABLE is not set"),
            },
        ))
    }

    fn kubernetes_nodes(min_nodes: i32, max_nodes: i32, cpu_archi: CpuArchitecture) -> Vec<NodeGroups> {
        let node_type = match cpu_archi {
            CpuArchitecture::AMD64 => "t3a.large".to_string(),
            CpuArchitecture::ARM64 => "m6g.xlarge".to_string(),
        };

        vec![
            NodeGroups::new("groupeks0".to_string(), min_nodes, max_nodes, node_type, 100, cpu_archi)
                .expect("Problem while setup EKS nodes"),
        ]
    }

    fn kubernetes_cluster_options(
        secrets: FuncTestsSecrets,
        _cluster_id: Option<String>,
        engine_location: EngineLocation,
        _vpc_network_mode: Option<VpcQoveryNetworkMode>,
    ) -> Options {
        Options {
            ec2_zone_a_subnet_blocks: vec!["10.0.0.0/20".to_string(), "10.0.16.0/20".to_string()],
            ec2_zone_b_subnet_blocks: vec!["10.0.32.0/20".to_string(), "10.0.48.0/20".to_string()],
            ec2_zone_c_subnet_blocks: vec!["10.0.64.0/20".to_string(), "10.0.80.0/20".to_string()],
            eks_zone_a_subnet_blocks: vec!["10.0.0.0/20".to_string(), "10.0.16.0/20".to_string()],
            eks_zone_b_subnet_blocks: vec!["10.0.32.0/20".to_string(), "10.0.48.0/20".to_string()],
            eks_zone_c_subnet_blocks: vec!["10.0.64.0/20".to_string(), "10.0.80.0/20".to_string()],
            rds_zone_a_subnet_blocks: vec![
                "10.0.214.0/23".to_string(),
                "10.0.216.0/23".to_string(),
                "10.0.218.0/23".to_string(),
                "10.0.220.0/23".to_string(),
                "10.0.222.0/23".to_string(),
                "10.0.224.0/23".to_string(),
            ],
            rds_zone_b_subnet_blocks: vec![
                "10.0.226.0/23".to_string(),
                "10.0.228.0/23".to_string(),
                "10.0.230.0/23".to_string(),
                "10.0.232.0/23".to_string(),
                "10.0.234.0/23".to_string(),
                "10.0.236.0/23".to_string(),
            ],
            rds_zone_c_subnet_blocks: vec![
                "10.0.238.0/23".to_string(),
                "10.0.240.0/23".to_string(),
                "10.0.242.0/23".to_string(),
                "10.0.244.0/23".to_string(),
                "10.0.246.0/23".to_string(),
                "10.0.248.0/23".to_string(),
            ],
            documentdb_zone_a_subnet_blocks: vec![
                "10.0.196.0/23".to_string(),
                "10.0.198.0/23".to_string(),
                "10.0.200.0/23".to_string(),
            ],
            documentdb_zone_b_subnet_blocks: vec![
                "10.0.202.0/23".to_string(),
                "10.0.204.0/23".to_string(),
                "10.0.206.0/23".to_string(),
            ],
            documentdb_zone_c_subnet_blocks: vec![
                "10.0.208.0/23".to_string(),
                "10.0.210.0/23".to_string(),
                "10.0.212.0/23".to_string(),
            ],
            elasticache_zone_a_subnet_blocks: vec!["10.0.172.0/23".to_string(), "10.0.174.0/23".to_string()],
            elasticache_zone_b_subnet_blocks: vec!["10.0.176.0/23".to_string(), "10.0.178.0/23".to_string()],
            elasticache_zone_c_subnet_blocks: vec!["10.0.180.0/23".to_string(), "10.0.182.0/23".to_string()],
            fargate_profile_zone_a_subnet_blocks: vec!["10.0.166.0/24".to_string()],
            fargate_profile_zone_b_subnet_blocks: vec!["10.0.168.0/24".to_string()],
            fargate_profile_zone_c_subnet_blocks: vec!["10.0.170.0/24".to_string()],
            eks_zone_a_nat_gw_for_fargate_subnet_blocks_public: vec!["10.0.132.0/22".to_string()],
            vpc_qovery_network_mode: VpcQoveryNetworkMode::WithoutNatGateways,
            vpc_cidr_block: "10.0.0.0/16".to_string(),
            eks_cidr_subnet: "20".to_string(),
            ec2_cidr_subnet: "20".to_string(),
            vpc_custom_routing_table: vec![],
            rds_cidr_subnet: "23".to_string(),
            documentdb_cidr_subnet: "23".to_string(),
            elasticache_cidr_subnet: "23".to_string(),
            qovery_api_url: secrets.QOVERY_API_URL.unwrap(),
            qovery_engine_location: engine_location,
            grafana_admin_user: "admin".to_string(),
            grafana_admin_password: "qovery".to_string(),
            qovery_ssh_key: secrets.QOVERY_SSH_USER.unwrap(),
            tls_email_report: secrets.LETS_ENCRYPT_EMAIL_REPORT.unwrap(),
            qovery_grpc_url: secrets.QOVERY_GRPC_URL.clone().unwrap(),
            qovery_engine_url: secrets.ENGINE_SERVER_URL.unwrap(),
            jwt_token: secrets.QOVERY_CLUSTER_JWT_TOKEN.unwrap(),
            user_ssh_keys: vec![],
            user_provided_network: None,
            aws_addon_cni_version_override: None,
            aws_addon_ebs_csi_version_override: None,
            aws_addon_kube_proxy_version_override: None,
            aws_addon_coredns_version_override: None,
            ec2_exposed_port: Some(9876),
            karpenter_parameters: None,
            metrics_parameters: None,
        }
    }
}

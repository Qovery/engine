extern crate serde;
extern crate serde_derive;

use const_format::formatcp;
use qovery_engine::cloud_provider::aws::kubernetes::{Options, VpcQoveryNetworkMode};
use qovery_engine::cloud_provider::aws::regions::AwsRegion;
use qovery_engine::cloud_provider::aws::AWS;
use qovery_engine::cloud_provider::kubernetes::Kind as KKind;
use qovery_engine::cloud_provider::models::NodeGroups;
use qovery_engine::cloud_provider::qovery::EngineLocation::ClientSide;
use qovery_engine::cloud_provider::Kind::Aws;
use qovery_engine::cloud_provider::{CloudProvider, TerraformStateCredentials};
use qovery_engine::container_registry::ecr::ECR;
use qovery_engine::dns_provider::DnsProvider;
use qovery_engine::engine::EngineConfig;
use qovery_engine::io_models::{Context, NoOpProgressListener};
use qovery_engine::logger::Logger;
use std::str::FromStr;
use std::sync::Arc;
use tracing::error;

use crate::cloudflare::dns_provider_cloudflare;
use crate::common::{get_environment_test_kubernetes, Cluster, ClusterDomain};
use crate::utilities::{build_platform_local_docker, FuncTestsSecrets};

pub const AWS_REGION_FOR_S3: AwsRegion = AwsRegion::EuWest3;
pub const AWS_TEST_REGION: AwsRegion = AwsRegion::EuWest3;
pub const AWS_KUBERNETES_MAJOR_VERSION: u8 = 1;
pub const AWS_KUBERNETES_MINOR_VERSION: u8 = 19;
pub const AWS_KUBERNETES_VERSION: &'static str =
    formatcp!("{}.{}", AWS_KUBERNETES_MAJOR_VERSION, AWS_KUBERNETES_MINOR_VERSION);
pub const AWS_DATABASE_INSTANCE_TYPE: &str = "db.t3.micro";
pub const AWS_DATABASE_DISK_TYPE: &str = "gp2";
pub const AWS_RESOURCE_TTL_IN_SECONDS: u32 = 7200;
pub const K3S_KUBERNETES_MAJOR_VERSION: u8 = 1;
pub const K3S_KUBERNETES_MINOR_VERSION: u8 = 20;

pub fn container_registry_ecr(context: &Context, logger: Box<dyn Logger>) -> ECR {
    let secrets = FuncTestsSecrets::new();
    if secrets.AWS_ACCESS_KEY_ID.is_none()
        || secrets.AWS_SECRET_ACCESS_KEY.is_none()
        || secrets.AWS_DEFAULT_REGION.is_none()
    {
        error!("Please check your Vault connectivity (token/address) or AWS_ACCESS_KEY_ID/AWS_SECRET_ACCESS_KEY/AWS_DEFAULT_REGION envrionment variables are set");
        std::process::exit(1)
    }

    ECR::new(
        context.clone(),
        "default-ecr-registry-Qovery Test",
        "ea59qe62xaw3wjai",
        secrets.AWS_ACCESS_KEY_ID.unwrap().as_str(),
        secrets.AWS_SECRET_ACCESS_KEY.unwrap().as_str(),
        secrets.AWS_DEFAULT_REGION.unwrap().as_str(),
        Arc::new(Box::new(NoOpProgressListener {})),
        logger,
    )
    .unwrap()
}

pub fn aws_default_engine_config(context: &Context, logger: Box<dyn Logger>) -> EngineConfig {
    AWS::docker_cr_engine(
        &context,
        logger,
        AWS_TEST_REGION.to_string().as_str(),
        KKind::Eks,
        AWS_KUBERNETES_VERSION.to_string(),
        &ClusterDomain::Default,
        None,
    )
}

impl Cluster<AWS, Options> for AWS {
    fn docker_cr_engine(
        context: &Context,
        logger: Box<dyn Logger>,
        localisation: &str,
        kubernetes_kind: KKind,
        kubernetes_version: String,
        cluster_domain: &ClusterDomain,
        vpc_network_mode: Option<VpcQoveryNetworkMode>,
    ) -> EngineConfig {
        // use ECR
        let container_registry = Box::new(container_registry_ecr(context, logger.clone()));

        // use LocalDocker
        let build_platform = Box::new(build_platform_local_docker(context, logger.clone()));

        // use AWS
        let cloud_provider: Arc<Box<dyn CloudProvider>> = Arc::new(AWS::cloud_provider(context));
        let dns_provider: Arc<Box<dyn DnsProvider>> = Arc::new(dns_provider_cloudflare(context, cluster_domain));

        let kubernetes = get_environment_test_kubernetes(
            Aws,
            context,
            cloud_provider.clone(),
            kubernetes_kind,
            dns_provider.clone(),
            logger.clone(),
            localisation,
            kubernetes_version.as_str(),
            vpc_network_mode,
        );

        EngineConfig::new(
            context.clone(),
            build_platform,
            container_registry,
            cloud_provider,
            dns_provider,
            kubernetes,
        )
    }

    fn cloud_provider(context: &Context) -> Box<AWS> {
        let secrets = FuncTestsSecrets::new();
        let aws_region =
            AwsRegion::from_str(secrets.AWS_DEFAULT_REGION.unwrap().as_str()).expect("AWS region not supported");
        Box::new(AWS::new(
            context.clone(),
            "u8nb94c7fwxzr2jt",
            secrets
                .AWS_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("AWS_TEST_ORGANIZATION_ID is not set")
                .as_str(),
            uuid::Uuid::new_v4(),
            "QoveryTest",
            secrets
                .AWS_ACCESS_KEY_ID
                .expect("AWS_ACCESS_KEY_ID is not set")
                .as_str(),
            secrets
                .AWS_SECRET_ACCESS_KEY
                .expect("AWS_SECRET_ACCESS_KEY is not set")
                .as_str(),
            aws_region.get_zones_to_string(),
            TerraformStateCredentials {
                access_key_id: secrets
                    .TERRAFORM_AWS_ACCESS_KEY_ID
                    .expect("TERRAFORM_AWS_ACCESS_KEY_ID is n ot set"),
                secret_access_key: secrets
                    .TERRAFORM_AWS_SECRET_ACCESS_KEY
                    .expect("TERRAFORM_AWS_SECRET_ACCESS_KEY is not set"),
                region: "eu-west-3".to_string(),
            },
        ))
    }

    fn kubernetes_nodes() -> Vec<NodeGroups> {
        vec![
            NodeGroups::new("groupeks0".to_string(), 5, 10, "t3a.large".to_string(), 100)
                .expect("Problem while setup EKS nodes"),
        ]
    }

    fn kubernetes_cluster_options(secrets: FuncTestsSecrets, _cluster_name: Option<String>) -> Options {
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
            elasticsearch_zone_a_subnet_blocks: vec!["10.0.184.0/23".to_string(), "10.0.186.0/23".to_string()],
            elasticsearch_zone_b_subnet_blocks: vec!["10.0.188.0/23".to_string(), "10.0.190.0/23".to_string()],
            elasticsearch_zone_c_subnet_blocks: vec!["10.0.192.0/23".to_string(), "10.0.194.0/23".to_string()],
            vpc_qovery_network_mode: VpcQoveryNetworkMode::WithoutNatGateways,
            vpc_cidr_block: "10.0.0.0/16".to_string(),
            eks_cidr_subnet: "20".to_string(),
            ec2_cidr_subnet: "20".to_string(),
            vpc_custom_routing_table: vec![],
            eks_access_cidr_blocks: secrets
                .EKS_ACCESS_CIDR_BLOCKS
                .as_ref()
                .unwrap()
                .replace("\"", "")
                .replace("[", "")
                .replace("]", "")
                .split(",")
                .map(|c| c.to_string())
                .collect(),
            ec2_access_cidr_blocks: secrets
                .EKS_ACCESS_CIDR_BLOCKS // FIXME ? use an EC2_ACCESS_CIDR_BLOCKS?
                .unwrap()
                .replace("\"", "")
                .replace("[", "")
                .replace("]", "")
                .split(",")
                .map(|c| c.to_string())
                .collect(),
            rds_cidr_subnet: "23".to_string(),
            documentdb_cidr_subnet: "23".to_string(),
            elasticache_cidr_subnet: "23".to_string(),
            elasticsearch_cidr_subnet: "23".to_string(),
            qovery_api_url: secrets.QOVERY_API_URL.unwrap(),
            qovery_engine_location: ClientSide,
            engine_version_controller_token: secrets.QOVERY_ENGINE_CONTROLLER_TOKEN.unwrap(),
            agent_version_controller_token: secrets.QOVERY_AGENT_CONTROLLER_TOKEN.unwrap(),
            grafana_admin_user: "admin".to_string(),
            grafana_admin_password: "qovery".to_string(),
            discord_api_key: secrets.DISCORD_API_URL.unwrap(),
            qovery_nats_url: secrets.QOVERY_NATS_URL.unwrap(),
            qovery_ssh_key: secrets.QOVERY_SSH_USER.unwrap(),
            qovery_nats_user: secrets.QOVERY_NATS_USERNAME.unwrap(),
            qovery_nats_password: secrets.QOVERY_NATS_PASSWORD.unwrap(),
            tls_email_report: secrets.LETS_ENCRYPT_EMAIL_REPORT.unwrap(),
            qovery_grpc_url: secrets.QOVERY_GRPC_URL.unwrap(),
            qovery_cluster_secret_token: secrets.QOVERY_CLUSTER_SECRET_TOKEN.unwrap(),
        }
    }
}

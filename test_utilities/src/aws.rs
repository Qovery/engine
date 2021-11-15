extern crate serde;
extern crate serde_derive;
use const_format::formatcp;
use qovery_engine::cloud_provider::aws::kubernetes::{EKSOptions, VpcQoveryNetworkMode};
use qovery_engine::cloud_provider::aws::AWS;
use qovery_engine::cloud_provider::kubernetes::{Cluster, ProviderOptions};
use qovery_engine::cloud_provider::models::NodeGroups;
use qovery_engine::cloud_provider::qovery::EngineLocation::ClientSide;
use qovery_engine::cloud_provider::TerraformStateCredentials;
use qovery_engine::container_registry::docker_hub::DockerHub;
use qovery_engine::container_registry::ecr::ECR;
use qovery_engine::dns_provider::DnsProvider;
use qovery_engine::engine::Engine;
use qovery_engine::models::{Context, EnvironmentAction};
use qovery_engine::transaction::{DeploymentOption, TransactionResult};
use tracing::error;

use crate::cloudflare::dns_provider_cloudflare;
use crate::common::Cluster;
use crate::utilities::{build_platform_local_docker, FuncTestsSecrets};

pub const AWS_QOVERY_ORGANIZATION_ID: &str = "u8nb94c7fwxzr2jt";
pub const AWS_REGION_FOR_S3: &str = "eu-west-3";
pub const AWS_KUBERNETES_MAJOR_VERSION: u8 = 1;
pub const AWS_KUBERNETES_MINOR_VERSION: u8 = 18;
pub const AWS_KUBERNETES_VERSION: &'static str =
    formatcp!("{}.{}", AWS_KUBERNETES_MAJOR_VERSION, AWS_KUBERNETES_MINOR_VERSION);
pub const AWS_KUBE_TEST_CLUSTER_ID: &str = "dmubm9agk7sr8a8r";
pub const AWS_DATABASE_INSTANCE_TYPE: &str = "db.t2.micro";
pub const AWS_DATABASE_DISK_TYPE: &str = "gp2";

pub fn container_registry_ecr(context: &Context) -> ECR {
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
    )
}

pub fn container_registry_docker_hub(context: &Context) -> DockerHub {
    DockerHub::new(
        context.clone(),
        "my-docker-hub-id-123",
        "my-default-docker-hub",
        "qoveryrd",
        "3b9481fe-74e7-4d7b-bc08-e147c9fd4f24",
    )
}

impl Cluster<AWS, EKSOptions> for AWS {
    fn docker_cr_engine(context: &Context) -> Engine {
        // use ECR
        let container_registry = Box::new(container_registry_ecr(context));

        // use LocalDocker
        let build_platform = Box::new(build_platform_local_docker(context));

        // use AWS
        let cloud_provider = AWS::cloud_provider(context);

        let dns_provider = Box::new(dns_provider_cloudflare(context));

        Engine::new(
            context.clone(),
            build_platform,
            container_registry,
            cloud_provider,
            dns_provider,
        )
    }

    fn cloud_provider(context: &Context) -> Box<AWS> {
        let secrets = FuncTestsSecrets::new();
        Box::new(AWS::new(
            context.clone(),
            "u8nb94c7fwxzr2jt",
            AWS_QOVERY_ORGANIZATION_ID,
            uuid::Uuid::new_v4(),
            "QoveryTest",
            secrets.AWS_ACCESS_KEY_ID.unwrap().as_str(),
            secrets.AWS_SECRET_ACCESS_KEY.unwrap().as_str(),
            TerraformStateCredentials {
                access_key_id: secrets.TERRAFORM_AWS_ACCESS_KEY_ID.unwrap(),
                secret_access_key: secrets.TERRAFORM_AWS_SECRET_ACCESS_KEY.unwrap(),
                region: "eu-west-3".to_string(),
            },
        ))
    }

    fn kubernetes_nodes() -> Vec<NodeGroups> {
        vec![NodeGroups::new("groupeks0".to_string(), 5, 10, "t3a.large".to_string())
            .expect("Problem while setup EKS nodes")]
    }

    fn kubernetes_cluster_options(secrets: FuncTestsSecrets, _cluster_name: Option<String>) -> EKSOptions {
        EKSOptions {
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
            eks_access_cidr_blocks: secrets
                .EKS_ACCESS_CIDR_BLOCKS
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

pub fn docker_ecr_aws_engine(context: &Context) -> Engine {
    // use ECR
    let container_registry = Box::new(container_registry_ecr(context));

    // use LocalDocker
    let build_platform = Box::new(build_platform_local_docker(context));

    // use AWS
    let cloud_provider = Box::new(cloud_provider_aws(context));

    let dns_provider = Box::new(dns_provider_cloudflare(context));

    Engine::new(
        context.clone(),
        build_platform,
        container_registry,
        cloud_provider,
        dns_provider,
    )
}

pub fn deploy_environment(context: &Context, environment_action: EnvironmentAction) -> TransactionResult {
    let engine = docker_ecr_aws_engine(context);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let cp = cloud_provider_aws(context);
    let nodes = aws_kubernetes_nodes();
    let dns_provider = dns_provider_cloudflare(context);
    let eks = aws_kubernetes_eks(context, &cp, &dns_provider, nodes);

    let _ = tx.deploy_environment_with_options(
        &eks,
        &environment_action,
        DeploymentOption {
            force_build: true,
            force_push: true,
        },
    );

    tx.commit()
}

pub fn delete_environment(context: &Context, environment_action: EnvironmentAction) -> TransactionResult {
    let engine = docker_ecr_aws_engine(context);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let cp = cloud_provider_aws(context);
    let nodes = aws_kubernetes_nodes();
    let dns_provider = dns_provider_cloudflare(context);
    let eks = aws_kubernetes_eks(context, &cp, &dns_provider, nodes);

    let _ = tx.delete_environment(&eks, &environment_action);

    tx.commit()
}

pub fn pause_environment(context: &Context, environment_action: EnvironmentAction) -> TransactionResult {
    let engine = docker_ecr_aws_engine(context);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let cp = cloud_provider_aws(context);
    let nodes = aws_kubernetes_nodes();
    let dns_provider = dns_provider_cloudflare(context);
    let eks = aws_kubernetes_eks(context, &cp, &dns_provider, nodes);
    let _ = tx.pause_environment(&eks, &environment_action);

    tx.commit()
}

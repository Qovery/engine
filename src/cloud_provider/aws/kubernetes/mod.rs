use core::fmt;
use std::io::Read;
use std::path::Path;
use std::str::FromStr;
use std::{env, fs};

use retry::delay::Fixed;
use retry::Error::Operation;
use retry::{Error, OperationResult};
use rusoto_core::credential::StaticProvider;
use rusoto_core::{Client, HttpClient, Region as RusotoRegion};
use rusoto_eks::{DescribeNodegroupRequest, Eks, EksClient, ListNodegroupsRequest, NodegroupScalingConfig};
use serde::{Deserialize, Serialize};
use tera::Context as TeraContext;

use crate::cloud_provider::aws::kubernetes::ec2_helm_charts::{
    ec2_aws_helm_charts, get_aws_ec2_qovery_terraform_config, Ec2ChartsConfigPrerequisites,
};
use crate::cloud_provider::aws::kubernetes::eks_helm_charts::{eks_aws_helm_charts, EksChartsConfigPrerequisites};
use crate::cloud_provider::aws::kubernetes::roles::get_default_roles_to_create;
use crate::cloud_provider::aws::kubernetes::vault::{ClusterSecretsAws, ClusterSecretsIoAws};
use crate::cloud_provider::aws::regions::{AwsRegion, AwsZones};
use crate::cloud_provider::helm::{deploy_charts_levels, ChartInfo};
use crate::cloud_provider::kubernetes::{
    is_kubernetes_upgrade_required, uninstall_cert_manager, Kind, Kubernetes, ProviderOptions,
};
use crate::cloud_provider::models::{
    KubernetesClusterAction, NodeGroups, NodeGroupsFormat, NodeGroupsWithDesiredState,
};
use crate::cloud_provider::qovery::EngineLocation;
use crate::cloud_provider::utilities::{wait_until_port_is_open, TcpCheckSource};
use crate::cloud_provider::CloudProvider;
use crate::cmd::helm::{to_engine_error, Helm};
use crate::cmd::kubectl::{kubectl_exec_api_custom_metrics, kubectl_exec_get_all_namespaces, kubectl_exec_get_events};
use crate::cmd::kubectl_utils::kubectl_are_qovery_infra_pods_executed;
use crate::cmd::terraform::{
    terraform_apply_with_tf_workers_resources, terraform_init_validate_plan_apply, terraform_init_validate_state_list,
    TerraformError,
};
use crate::deletion_utilities::{get_firsts_namespaces_to_delete, get_qovery_managed_namespaces};
use crate::dns_provider::DnsProvider;
use crate::errors::{CommandError, EngineError, ErrorMessageVerbosity};
use crate::events::{EngineEvent, EventDetails, EventMessage, InfrastructureStep, Stage, Transmitter};
use crate::io_models::context::{Context, Features};
use crate::io_models::domain::{ToHelmString, ToTerraformString};
use crate::io_models::QoveryIdentifier;
use crate::object_storage::s3::S3;
use crate::runtime::block_on;
use crate::secret_manager::vault::QVaultClient;
use crate::string::terraform_list_format;
use crate::{cmd, secret_manager};

use self::eks::select_nodegroups_autoscaling_group_behavior;

pub mod ec2;
mod ec2_helm_charts;
pub mod eks;
pub mod eks_helm_charts;
pub mod node;
pub mod roles;
mod vault;

// https://docs.aws.amazon.com/eks/latest/userguide/external-snat.html
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VpcQoveryNetworkMode {
    WithNatGateways,
    WithoutNatGateways,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpcCustomRoutingTable {
    description: String,
    destination: String,
    target: String,
}

impl fmt::Display for VpcQoveryNetworkMode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Options {
    // AWS related
    #[serde(default)] // TODO: remove default
    pub ec2_zone_a_subnet_blocks: Vec<String>,
    #[serde(default)] // TODO: remove default
    pub ec2_zone_b_subnet_blocks: Vec<String>,
    #[serde(default)] // TODO: remove default
    pub ec2_zone_c_subnet_blocks: Vec<String>,
    pub eks_zone_a_subnet_blocks: Vec<String>,
    pub eks_zone_b_subnet_blocks: Vec<String>,
    pub eks_zone_c_subnet_blocks: Vec<String>,
    pub rds_zone_a_subnet_blocks: Vec<String>,
    pub rds_zone_b_subnet_blocks: Vec<String>,
    pub rds_zone_c_subnet_blocks: Vec<String>,
    pub documentdb_zone_a_subnet_blocks: Vec<String>,
    pub documentdb_zone_b_subnet_blocks: Vec<String>,
    pub documentdb_zone_c_subnet_blocks: Vec<String>,
    pub elasticache_zone_a_subnet_blocks: Vec<String>,
    pub elasticache_zone_b_subnet_blocks: Vec<String>,
    pub elasticache_zone_c_subnet_blocks: Vec<String>,
    pub elasticsearch_zone_a_subnet_blocks: Vec<String>,
    pub elasticsearch_zone_b_subnet_blocks: Vec<String>,
    pub elasticsearch_zone_c_subnet_blocks: Vec<String>,
    pub vpc_qovery_network_mode: VpcQoveryNetworkMode,
    pub vpc_cidr_block: String,
    pub eks_cidr_subnet: String,
    #[serde(default)] // TODO: remove default
    pub ec2_cidr_subnet: String,
    pub vpc_custom_routing_table: Vec<VpcCustomRoutingTable>,
    pub eks_access_cidr_blocks: Vec<String>,
    #[serde(default)] // TODO: remove default
    pub ec2_access_cidr_blocks: Vec<String>,
    pub rds_cidr_subnet: String,
    pub documentdb_cidr_subnet: String,
    pub elasticache_cidr_subnet: String,
    pub elasticsearch_cidr_subnet: String,
    // Qovery
    pub qovery_api_url: String,
    pub qovery_grpc_url: String,
    pub jwt_token: String,
    pub qovery_engine_location: EngineLocation,
    pub engine_version_controller_token: String,
    pub agent_version_controller_token: String,
    pub grafana_admin_user: String,
    pub grafana_admin_password: String,
    pub discord_api_key: String,
    pub qovery_nats_url: String,
    pub qovery_nats_user: String,
    pub qovery_nats_password: String,
    pub qovery_ssh_key: String,
    #[serde(default)]
    pub user_ssh_keys: Vec<String>,
    // Others
    pub tls_email_report: String,
    #[serde(default)]
    pub user_network_config: Option<UserNetworkConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserNetworkConfig {
    pub documentdb_subnets_zone_a_ids: Vec<String>,
    pub documentdb_subnets_zone_b_ids: Vec<String>,
    pub documentdb_subnets_zone_c_ids: Vec<String>,

    pub elasticache_subnets_zone_a_ids: Vec<String>,
    pub elasticache_subnets_zone_b_ids: Vec<String>,
    pub elasticache_subnets_zone_c_ids: Vec<String>,

    pub elasticsearch_subnets_zone_a_ids: Vec<String>,
    pub elasticsearch_subnets_zone_b_ids: Vec<String>,
    pub elasticsearch_subnets_zone_c_ids: Vec<String>,

    pub rds_subnets_zone_a_ids: Vec<String>,
    pub rds_subnets_zone_b_ids: Vec<String>,
    pub rds_subnets_zone_c_ids: Vec<String>,

    // must have enable_dns_hostnames = true
    pub aws_vpc_eks_id: String,

    // must have map_public_ip_on_launch = true
    pub eks_subnets_zone_a_ids: Vec<String>,
    pub eks_subnets_zone_b_ids: Vec<String>,
    pub eks_subnets_zone_c_ids: Vec<String>,
}

impl ProviderOptions for Options {}

fn event_details<S: Into<String>>(
    cloud_provider: &dyn CloudProvider,
    kubernetes_id: S,
    kubernetes_name: S,
    kubernetes_region: &AwsRegion,
    context: &Context,
) -> EventDetails {
    EventDetails::new(
        Some(cloud_provider.kind()),
        QoveryIdentifier::new_from_long_id(context.organization_id().to_string()),
        QoveryIdentifier::new_from_long_id(context.cluster_id().to_string()),
        QoveryIdentifier::new_from_long_id(context.execution_id().to_string()),
        Some(kubernetes_region.to_string()),
        Stage::Infrastructure(InfrastructureStep::LoadConfiguration),
        Transmitter::Kubernetes(kubernetes_id.into(), kubernetes_name.into()),
    )
}

fn aws_zones(
    zones: Vec<String>,
    region: &AwsRegion,
    event_details: &EventDetails,
) -> Result<Vec<AwsZones>, EngineError> {
    let mut aws_zones = vec![];

    for zone in zones {
        match AwsZones::from_string(zone.to_string()) {
            Ok(x) => aws_zones.push(x),
            Err(e) => {
                return Err(EngineError::new_unsupported_zone(
                    event_details.clone(),
                    region.to_string(),
                    zone,
                    CommandError::new_from_safe_message(e.to_string()),
                ));
            }
        };
    }

    Ok(aws_zones)
}

fn s3(context: &Context, region: &AwsRegion, cloud_provider: &dyn CloudProvider, ttl: i32) -> S3 {
    let bucket_ttl = match ttl {
        0 => None,
        _ => Some(ttl),
    };
    S3::new(
        context.clone(),
        "s3-temp-id".to_string(),
        "default-s3".to_string(),
        cloud_provider.access_key_id(),
        cloud_provider.secret_access_key(),
        region.clone(),
        true,
        bucket_ttl,
    )
}

/// divide by 2 the total number of subnet to get the exact same number as private and public
fn check_odd_subnets(
    event_details: EventDetails,
    zone_name: &str,
    subnet_block: &[String],
) -> Result<usize, EngineError> {
    if subnet_block.len() % 2 == 1 {
        return Err(EngineError::new_subnets_count_is_not_even(
            event_details,
            zone_name.to_string(),
            subnet_block.len(),
        ));
    }

    Ok((subnet_block.len() / 2) as usize)
}

fn lets_encrypt_url(context: &Context) -> String {
    match context.is_test_cluster() {
        true => "https://acme-staging-v02.api.letsencrypt.org/directory",
        false => "https://acme-v02.api.letsencrypt.org/directory",
    }
    .to_string()
}

fn managed_dns_resolvers_terraform_format(dns_provider: &dyn DnsProvider) -> String {
    let managed_dns_resolvers = dns_provider
        .resolvers()
        .iter()
        .map(|x| format!("{}", x.clone()))
        .collect::<Vec<_>>();

    terraform_list_format(managed_dns_resolvers)
}

fn tera_context(
    kubernetes: &dyn Kubernetes,
    zones: &[AwsZones],
    node_groups: &[NodeGroupsWithDesiredState],
    options: &Options,
) -> Result<TeraContext, EngineError> {
    let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::LoadConfiguration));
    let mut context = TeraContext::new();

    context.insert("user_provided_network", &false);
    if let Some(user_network_cfg) = &options.user_network_config {
        context.insert("user_provided_network", &true);

        context.insert("documentdb_subnets_zone_a_ids", &user_network_cfg.documentdb_subnets_zone_a_ids);
        context.insert("documentdb_subnets_zone_b_ids", &user_network_cfg.documentdb_subnets_zone_b_ids);
        context.insert("documentdb_subnets_zone_c_ids", &user_network_cfg.documentdb_subnets_zone_c_ids);

        context.insert(
            "elasticache_subnets_zone_a_ids",
            &user_network_cfg.elasticache_subnets_zone_a_ids,
        );
        context.insert(
            "elasticache_subnets_zone_b_ids",
            &user_network_cfg.elasticache_subnets_zone_b_ids,
        );
        context.insert(
            "elasticache_subnets_zone_c_ids",
            &user_network_cfg.elasticache_subnets_zone_c_ids,
        );

        context.insert(
            "elasticsearch_subnets_zone_a_ids",
            &user_network_cfg.elasticsearch_subnets_zone_a_ids,
        );
        context.insert(
            "elasticsearch_subnets_zone_b_ids",
            &user_network_cfg.elasticsearch_subnets_zone_b_ids,
        );
        context.insert(
            "elasticsearch_subnets_zone_c_ids",
            &user_network_cfg.elasticsearch_subnets_zone_c_ids,
        );

        context.insert("rds_subnets_zone_a_ids", &user_network_cfg.rds_subnets_zone_a_ids);
        context.insert("rds_subnets_zone_b_ids", &user_network_cfg.rds_subnets_zone_b_ids);
        context.insert("rds_subnets_zone_c_ids", &user_network_cfg.rds_subnets_zone_c_ids);

        context.insert("aws_vpc_eks_id", &user_network_cfg.aws_vpc_eks_id);

        context.insert("eks_subnets_zone_a_ids", &user_network_cfg.eks_subnets_zone_a_ids);
        context.insert("eks_subnets_zone_b_ids", &user_network_cfg.eks_subnets_zone_b_ids);
        context.insert("eks_subnets_zone_c_ids", &user_network_cfg.eks_subnets_zone_c_ids);
    }

    let format_ips =
        |ips: &Vec<String>| -> Vec<String> { ips.iter().map(|ip| format!("\"{}\"", ip)).collect::<Vec<_>>() };

    let aws_zones = zones
        .iter()
        .map(|zone| zone.to_terraform_format_string())
        .collect::<Vec<_>>();

    let mut eks_zone_a_subnet_blocks_private = format_ips(&options.eks_zone_a_subnet_blocks);
    let mut eks_zone_b_subnet_blocks_private = format_ips(&options.eks_zone_b_subnet_blocks);
    let mut eks_zone_c_subnet_blocks_private = format_ips(&options.eks_zone_c_subnet_blocks);

    match options.vpc_qovery_network_mode {
        VpcQoveryNetworkMode::WithNatGateways => {
            let max_subnet_zone_a = check_odd_subnets(event_details.clone(), "a", &eks_zone_a_subnet_blocks_private)?;
            let max_subnet_zone_b = check_odd_subnets(event_details.clone(), "b", &eks_zone_b_subnet_blocks_private)?;
            let max_subnet_zone_c = check_odd_subnets(event_details.clone(), "c", &eks_zone_c_subnet_blocks_private)?;

            let eks_zone_a_subnet_blocks_public: Vec<String> =
                eks_zone_a_subnet_blocks_private.drain(max_subnet_zone_a..).collect();
            let eks_zone_b_subnet_blocks_public: Vec<String> =
                eks_zone_b_subnet_blocks_private.drain(max_subnet_zone_b..).collect();
            let eks_zone_c_subnet_blocks_public: Vec<String> =
                eks_zone_c_subnet_blocks_private.drain(max_subnet_zone_c..).collect();

            context.insert("eks_zone_a_subnet_blocks_public", &eks_zone_a_subnet_blocks_public);
            context.insert("eks_zone_b_subnet_blocks_public", &eks_zone_b_subnet_blocks_public);
            context.insert("eks_zone_c_subnet_blocks_public", &eks_zone_c_subnet_blocks_public);
        }
        VpcQoveryNetworkMode::WithoutNatGateways => {}
    };

    let mut ec2_zone_a_subnet_blocks_private = format_ips(&options.ec2_zone_a_subnet_blocks);
    let mut ec2_zone_b_subnet_blocks_private = format_ips(&options.ec2_zone_b_subnet_blocks);
    let mut ec2_zone_c_subnet_blocks_private = format_ips(&options.ec2_zone_c_subnet_blocks);

    match options.vpc_qovery_network_mode {
        VpcQoveryNetworkMode::WithNatGateways => {
            let max_subnet_zone_a = check_odd_subnets(event_details.clone(), "a", &ec2_zone_a_subnet_blocks_private)?;
            let max_subnet_zone_b = check_odd_subnets(event_details.clone(), "b", &ec2_zone_b_subnet_blocks_private)?;
            let max_subnet_zone_c = check_odd_subnets(event_details.clone(), "c", &ec2_zone_c_subnet_blocks_private)?;

            let ec2_zone_a_subnet_blocks_public: Vec<String> =
                ec2_zone_a_subnet_blocks_private.drain(max_subnet_zone_a..).collect();
            let ec2_zone_b_subnet_blocks_public: Vec<String> =
                ec2_zone_b_subnet_blocks_private.drain(max_subnet_zone_b..).collect();
            let ec2_zone_c_subnet_blocks_public: Vec<String> =
                ec2_zone_c_subnet_blocks_private.drain(max_subnet_zone_c..).collect();

            context.insert("ec2_zone_a_subnet_blocks_public", &ec2_zone_a_subnet_blocks_public);
            context.insert("ec2_zone_b_subnet_blocks_public", &ec2_zone_b_subnet_blocks_public);
            context.insert("ec2_zone_c_subnet_blocks_public", &ec2_zone_c_subnet_blocks_public);
        }
        VpcQoveryNetworkMode::WithoutNatGateways => {}
    };

    context.insert("vpc_qovery_network_mode", &options.vpc_qovery_network_mode.to_string());

    let rds_zone_a_subnet_blocks = format_ips(&options.rds_zone_a_subnet_blocks);
    let rds_zone_b_subnet_blocks = format_ips(&options.rds_zone_b_subnet_blocks);
    let rds_zone_c_subnet_blocks = format_ips(&options.rds_zone_c_subnet_blocks);

    let documentdb_zone_a_subnet_blocks = format_ips(&options.documentdb_zone_a_subnet_blocks);
    let documentdb_zone_b_subnet_blocks = format_ips(&options.documentdb_zone_b_subnet_blocks);
    let documentdb_zone_c_subnet_blocks = format_ips(&options.documentdb_zone_c_subnet_blocks);

    let elasticache_zone_a_subnet_blocks = format_ips(&options.elasticache_zone_a_subnet_blocks);
    let elasticache_zone_b_subnet_blocks = format_ips(&options.elasticache_zone_b_subnet_blocks);
    let elasticache_zone_c_subnet_blocks = format_ips(&options.elasticache_zone_c_subnet_blocks);

    let elasticsearch_zone_a_subnet_blocks = format_ips(&options.elasticsearch_zone_a_subnet_blocks);
    let elasticsearch_zone_b_subnet_blocks = format_ips(&options.elasticsearch_zone_b_subnet_blocks);
    let elasticsearch_zone_c_subnet_blocks = format_ips(&options.elasticsearch_zone_c_subnet_blocks);

    let region_cluster_id = format!("{}-{}", kubernetes.region(), kubernetes.id());
    let vpc_cidr_block = options.vpc_cidr_block.clone();
    let eks_cloudwatch_log_group = format!("/aws/eks/{}/cluster", kubernetes.id());
    let eks_cidr_subnet = options.eks_cidr_subnet.clone();
    let ec2_cidr_subnet = options.ec2_cidr_subnet.clone();

    let eks_access_cidr_blocks = format_ips(&options.eks_access_cidr_blocks);
    let ec2_access_cidr_blocks = format_ips(&options.ec2_access_cidr_blocks);

    let qovery_api_url = options.qovery_api_url.clone();
    let rds_cidr_subnet = options.rds_cidr_subnet.clone();
    let documentdb_cidr_subnet = options.documentdb_cidr_subnet.clone();
    let elasticache_cidr_subnet = options.elasticache_cidr_subnet.clone();
    let elasticsearch_cidr_subnet = options.elasticsearch_cidr_subnet.clone();

    // Qovery
    context.insert("organization_id", kubernetes.cloud_provider().organization_id());
    context.insert("qovery_api_url", &qovery_api_url);

    context.insert("engine_version_controller_token", &options.engine_version_controller_token);
    context.insert("agent_version_controller_token", &options.agent_version_controller_token);

    context.insert("test_cluster", &kubernetes.context().is_test_cluster());

    context.insert("force_upgrade", &kubernetes.context().requires_forced_upgrade());

    // Qovery features
    context.insert(
        "log_history_enabled",
        &kubernetes.context().is_feature_enabled(&Features::LogsHistory),
    );
    context.insert(
        "metrics_history_enabled",
        &kubernetes.context().is_feature_enabled(&Features::MetricsHistory),
    );

    // DNS configuration
    let managed_dns_list = vec![kubernetes.dns_provider().name()];
    let managed_dns_domains_helm_format = vec![kubernetes.dns_provider().domain().to_string()];
    let managed_dns_domains_root_helm_format = vec![kubernetes.dns_provider().domain().root_domain().to_string()];
    let managed_dns_domains_terraform_format =
        terraform_list_format(vec![kubernetes.dns_provider().domain().to_string()]);
    let managed_dns_domains_root_terraform_format =
        terraform_list_format(vec![kubernetes.dns_provider().domain().root_domain().to_string()]);
    let managed_dns_resolvers_terraform_format = managed_dns_resolvers_terraform_format(kubernetes.dns_provider());

    context.insert("managed_dns", &managed_dns_list);
    context.insert("managed_dns_domains_helm_format", &managed_dns_domains_helm_format);
    context.insert("managed_dns_domains_root_helm_format", &managed_dns_domains_root_helm_format);
    context.insert("managed_dns_domains_terraform_format", &managed_dns_domains_terraform_format);
    context.insert(
        "managed_dns_domains_root_terraform_format",
        &managed_dns_domains_root_terraform_format,
    );
    context.insert(
        "managed_dns_resolvers_terraform_format",
        &managed_dns_resolvers_terraform_format,
    );

    context.insert(
        "wildcard_managed_dns",
        &kubernetes.dns_provider().domain().wildcarded().to_string(),
    );

    // add specific DNS fields
    kubernetes.dns_provider().insert_into_teracontext(&mut context);

    context.insert("dns_email_report", &options.tls_email_report);

    // TLS
    context.insert("acme_server_url", &lets_encrypt_url(kubernetes.context()));

    // Vault
    context.insert("vault_auth_method", "none");

    if env::var_os("VAULT_ADDR").is_some() {
        // select the correct used method
        match env::var_os("VAULT_ROLE_ID") {
            Some(role_id) => {
                context.insert("vault_auth_method", "app_role");
                context.insert("vault_role_id", role_id.to_str().unwrap());

                match env::var_os("VAULT_SECRET_ID") {
                    Some(secret_id) => context.insert("vault_secret_id", secret_id.to_str().unwrap()),
                    None => kubernetes.logger().log(EngineEvent::Error(
                        EngineError::new_missing_required_env_variable(event_details, "VAULT_SECRET_ID".to_string()),
                        None,
                    )),
                }
            }
            None => {
                if env::var_os("VAULT_TOKEN").is_some() {
                    context.insert("vault_auth_method", "token")
                }
            }
        }
    };

    // Other Kubernetes
    context.insert("kubernetes_cluster_name", &kubernetes.cluster_name());
    context.insert("enable_cluster_autoscaler", &true);

    // AWS
    context.insert("aws_access_key", &kubernetes.cloud_provider().access_key_id());
    context.insert("aws_secret_key", &kubernetes.cloud_provider().secret_access_key());

    // AWS S3 tfstate storage
    context.insert(
        "aws_access_key_tfstates_account",
        kubernetes
            .cloud_provider()
            .terraform_state_credentials()
            .access_key_id
            .as_str(),
    );

    context.insert(
        "aws_secret_key_tfstates_account",
        kubernetes
            .cloud_provider()
            .terraform_state_credentials()
            .secret_access_key
            .as_str(),
    );
    context.insert(
        "aws_region_tfstates_account",
        kubernetes
            .cloud_provider()
            .terraform_state_credentials()
            .region
            .as_str(),
    );

    context.insert("aws_region", &kubernetes.region());
    context.insert("aws_terraform_backend_bucket", "qovery-terrafom-tfstates");
    context.insert("aws_terraform_backend_dynamodb_table", "qovery-terrafom-tfstates");
    context.insert("vpc_cidr_block", &vpc_cidr_block);
    context.insert("vpc_custom_routing_table", &options.vpc_custom_routing_table);
    context.insert("s3_kubeconfig_bucket", &format!("qovery-kubeconfigs-{}", kubernetes.id()));

    // AWS - EKS
    context.insert("aws_availability_zones", &aws_zones);
    context.insert("eks_cidr_subnet", &eks_cidr_subnet);
    context.insert("ec2_cidr_subnet", &ec2_cidr_subnet);
    context.insert("kubernetes_cluster_name", kubernetes.name());
    context.insert("kubernetes_cluster_id", kubernetes.id());
    context.insert("kubernetes_full_cluster_id", kubernetes.context().cluster_id());
    context.insert("eks_region_cluster_id", region_cluster_id.as_str());
    context.insert("eks_worker_nodes", &node_groups);
    context.insert("ec2_zone_a_subnet_blocks_private", &ec2_zone_a_subnet_blocks_private);
    context.insert("ec2_zone_b_subnet_blocks_private", &ec2_zone_b_subnet_blocks_private);
    context.insert("ec2_zone_c_subnet_blocks_private", &ec2_zone_c_subnet_blocks_private);
    context.insert("eks_zone_a_subnet_blocks_private", &eks_zone_a_subnet_blocks_private);
    context.insert("eks_zone_b_subnet_blocks_private", &eks_zone_b_subnet_blocks_private);
    context.insert("eks_zone_c_subnet_blocks_private", &eks_zone_c_subnet_blocks_private);
    context.insert("eks_masters_version", &kubernetes.version());
    context.insert("eks_workers_version", &kubernetes.version());
    context.insert("ec2_masters_version", &kubernetes.version());
    context.insert("ec2_workers_version", &kubernetes.version());
    context.insert("eks_cloudwatch_log_group", &eks_cloudwatch_log_group);
    context.insert("eks_access_cidr_blocks", &eks_access_cidr_blocks);
    context.insert("ec2_access_cidr_blocks", &ec2_access_cidr_blocks);

    // AWS - RDS
    context.insert("rds_cidr_subnet", &rds_cidr_subnet);
    context.insert("rds_zone_a_subnet_blocks", &rds_zone_a_subnet_blocks);
    context.insert("rds_zone_b_subnet_blocks", &rds_zone_b_subnet_blocks);
    context.insert("rds_zone_c_subnet_blocks", &rds_zone_c_subnet_blocks);

    // AWS - DocumentDB
    context.insert("documentdb_cidr_subnet", &documentdb_cidr_subnet);
    context.insert("documentdb_zone_a_subnet_blocks", &documentdb_zone_a_subnet_blocks);
    context.insert("documentdb_zone_b_subnet_blocks", &documentdb_zone_b_subnet_blocks);
    context.insert("documentdb_zone_c_subnet_blocks", &documentdb_zone_c_subnet_blocks);

    // AWS - Elasticache
    context.insert("elasticache_cidr_subnet", &elasticache_cidr_subnet);
    context.insert("elasticache_zone_a_subnet_blocks", &elasticache_zone_a_subnet_blocks);
    context.insert("elasticache_zone_b_subnet_blocks", &elasticache_zone_b_subnet_blocks);
    context.insert("elasticache_zone_c_subnet_blocks", &elasticache_zone_c_subnet_blocks);

    // AWS - Elasticsearch
    context.insert("elasticsearch_cidr_subnet", &elasticsearch_cidr_subnet);
    context.insert("elasticsearch_zone_a_subnet_blocks", &elasticsearch_zone_a_subnet_blocks);
    context.insert("elasticsearch_zone_b_subnet_blocks", &elasticsearch_zone_b_subnet_blocks);
    context.insert("elasticsearch_zone_c_subnet_blocks", &elasticsearch_zone_c_subnet_blocks);

    // grafana credentials
    context.insert("grafana_admin_user", options.grafana_admin_user.as_str());
    context.insert("grafana_admin_password", options.grafana_admin_password.as_str());

    // qovery
    context.insert("qovery_api_url", options.qovery_api_url.as_str());
    context.insert("qovery_nats_url", options.qovery_nats_url.as_str());
    context.insert("qovery_nats_user", options.qovery_nats_user.as_str());
    context.insert("qovery_nats_password", options.qovery_nats_password.as_str());
    context.insert("qovery_ssh_key", options.qovery_ssh_key.as_str());
    // AWS support only 1 ssh key
    let user_ssh_key: Option<&str> = options.user_ssh_keys.get(0).map(|x| x.as_str());
    context.insert("user_ssh_key", user_ssh_key.unwrap_or_default());
    context.insert("discord_api_key", options.discord_api_key.as_str());

    // Advanced settings
    context.insert(
        "registry_image_retention_time",
        &kubernetes.advanced_settings().registry_image_retention_time_sec,
    );
    context.insert(
        "resource_expiration_in_seconds",
        &kubernetes.advanced_settings().pleco_resources_ttl,
    );

    Ok(context)
}

#[derive(Serialize, Deserialize)]
pub struct NodeGroupDesiredState {
    pub update_desired_nodes: bool,
    pub desired_nodes_count: i32,
}

impl NodeGroupDesiredState {
    pub fn new(update_desired_nodes: bool, desired_nodes_count: i32) -> NodeGroupDesiredState {
        NodeGroupDesiredState {
            update_desired_nodes,
            desired_nodes_count,
        }
    }
}

/// Returns a tuple of (update_desired_node: bool, desired_nodes_count: i32).
fn should_update_desired_nodes(
    event_details: EventDetails,
    kubernetes: &dyn Kubernetes,
    action: KubernetesClusterAction,
    node_groups: &[NodeGroups],
    aws_eks_client: Option<EksClient>,
) -> Result<Vec<NodeGroupsWithDesiredState>, EngineError> {
    let get_autoscaling_config = |node_group: &NodeGroups, eks_client: EksClient| -> Result<Option<i32>, EngineError> {
        let current_nodes = get_nodegroup_autoscaling_config_from_aws(
            event_details.clone(),
            kubernetes,
            node_group.clone(),
            eks_client,
        )?;
        match current_nodes {
            Some(x) => match x.desired_size {
                Some(n) => Ok(Some(n as i32)),
                None => Ok(None),
            },
            None => Ok(None),
        }
    };
    let mut node_groups_with_size = Vec::with_capacity(node_groups.len());

    for node_group in node_groups {
        let eks_client = match aws_eks_client.clone() {
            Some(x) => x,
            None => {
                // if no no clients, we're in bootstrap mode
                select_nodegroups_autoscaling_group_behavior(action, node_group);
                continue;
            }
        };
        let node_group_with_desired_state = match action {
            KubernetesClusterAction::Bootstrap | KubernetesClusterAction::Pause | KubernetesClusterAction::Delete => {
                select_nodegroups_autoscaling_group_behavior(action, node_group)
            }
            KubernetesClusterAction::Update(_) => {
                let current_nodes = get_autoscaling_config(node_group, eks_client)?;
                select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Update(current_nodes), node_group)
            }
            KubernetesClusterAction::Upgrade(_) => {
                let current_nodes = get_autoscaling_config(node_group, eks_client)?;
                select_nodegroups_autoscaling_group_behavior(
                    KubernetesClusterAction::Upgrade(current_nodes),
                    node_group,
                )
            }
            KubernetesClusterAction::Resume(_) => {
                let current_nodes = get_autoscaling_config(node_group, eks_client)?;
                select_nodegroups_autoscaling_group_behavior(KubernetesClusterAction::Resume(current_nodes), node_group)
            }
        };
        node_groups_with_size.push(node_group_with_desired_state)
    }

    Ok(node_groups_with_size)
}

/// Returns a rusoto eks client using the current configuration.
fn get_rusoto_eks_client(event_details: EventDetails, kubernetes: &dyn Kubernetes) -> Result<EksClient, EngineError> {
    let cloud_provider = kubernetes.cloud_provider();
    let region = match RusotoRegion::from_str(kubernetes.region()) {
        Ok(value) => value,
        Err(error) => {
            return Err(EngineError::new_unsupported_region(
                event_details,
                kubernetes.region().to_string(),
                CommandError::new_from_safe_message(error.to_string()),
            ));
        }
    };

    let credentials =
        StaticProvider::new(cloud_provider.access_key_id(), cloud_provider.secret_access_key(), None, None);

    let client = Client::new_with(credentials, HttpClient::new().expect("unable to create new Http client"));
    Ok(EksClient::new_with_client(client, region))
}

/// Returns the scaling config of a node_group by node_group_name.
fn get_nodegroup_autoscaling_config_from_aws(
    event_details: EventDetails,
    kubernetes: &dyn Kubernetes,
    node_group: NodeGroups,
    eks_client: EksClient,
) -> Result<Option<NodegroupScalingConfig>, EngineError> {
    // In case of EC2, there is no need to care about auto scaling
    if kubernetes.kind() == Kind::Ec2 {
        return Ok(None);
    }

    let eks_node_groups = match block_on(eks_client.list_nodegroups(ListNodegroupsRequest {
        cluster_name: kubernetes.cluster_name(),
        ..Default::default()
    })) {
        Ok(res) => match res.nodegroups {
            // This could be empty on paused clusters, we should not return an error for this
            None => return Ok(None),
            Some(x) => x,
        },
        Err(e) => {
            return Err(EngineError::new_nodegroup_list_error(
                event_details,
                CommandError::new(
                    e.to_string(),
                    Some("Error while trying to get node groups from eks".to_string()),
                    None,
                ),
            ))
        }
    };

    // Find eks_node_group that matches the node_group.name passed in parameters
    let mut scaling_config: Option<NodegroupScalingConfig> = None;
    for eks_node_group_name in eks_node_groups {
        // warn: can't filter the state of the autoscaling group with this lib. We should filter on running (and not deleting/creating)
        let eks_node_group = match block_on(eks_client.describe_nodegroup(DescribeNodegroupRequest {
            cluster_name: kubernetes.cluster_name(),
            nodegroup_name: eks_node_group_name,
        })) {
            Ok(res) => match res.nodegroup {
                None => return Err(EngineError::new_missing_nodegroup_information_error(event_details)),
                Some(x) => x,
            },
            Err(error) => {
                return Err(EngineError::new_cluster_worker_node_not_found(
                    event_details,
                    Some(CommandError::new(
                        "Error while trying to get node groups from AWS".to_string(),
                        Some(error.to_string()),
                        None,
                    )),
                ));
            }
        };
        // ignore if group of nodes is not managed by Qovery
        if eks_node_group.tags.unwrap_or_default()["QoveryNodeGroupName"] == node_group.name {
            scaling_config = eks_node_group.scaling_config;
            break;
        }
    }

    Ok(scaling_config)
}

fn create(
    kubernetes: &dyn Kubernetes,
    kubernetes_long_id: uuid::Uuid,
    template_directory: &str,
    aws_zones: &[AwsZones],
    node_groups: &[NodeGroups],
    options: &Options,
) -> Result<(), EngineError> {
    let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Create));
    let mut kubernetes_action = KubernetesClusterAction::Bootstrap;

    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Preparing EKS cluster deployment.".to_string()),
    ));

    // upgrade cluster instead if required
    if !kubernetes.context().is_first_cluster_deployment() {
        match kubernetes.get_kubeconfig_file() {
            Ok((path, _)) => match is_kubernetes_upgrade_required(
                path,
                kubernetes.version(),
                kubernetes.cloud_provider().credentials_environment_variables(),
                event_details.clone(),
                kubernetes.logger(),
            ) {
                Ok(x) => {
                    if x.required_upgrade_on.is_some() {
                        return kubernetes.upgrade_with_status(x);
                    }

                    kubernetes_action = KubernetesClusterAction::Update(None);

                    kubernetes.logger().log(EngineEvent::Info(
                        event_details.clone(),
                        EventMessage::new_from_safe("Kubernetes cluster upgrade not required".to_string()),
                    ))
                }
                Err(e) => {
                    kubernetes.logger().log(EngineEvent::Error(
                        e,
                        Some(EventMessage::new_from_safe(
                            "Error detected, upgrade won't occurs, but standard deployment.".to_string(),
                        )),
                    ));
                }
            },
            Err(_) => kubernetes.logger().log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe("Kubernetes cluster upgrade not required, config file is not found and cluster have certainly never been deployed before".to_string())))
        };
    };

    // create AWS IAM roles
    let already_created_roles = get_default_roles_to_create();
    for role in already_created_roles {
        match role.create_service_linked_role(
            kubernetes.cloud_provider().access_key_id().as_str(),
            kubernetes.cloud_provider().secret_access_key().as_str(),
        ) {
            Ok(_) => kubernetes.logger().log(EngineEvent::Info(
                event_details.clone(),
                EventMessage::new_from_safe(format!("Role {} is already present, no need to create", role.role_name)),
            )),
            Err(e) => kubernetes.logger().log(EngineEvent::Error(
                EngineError::new_cannot_get_or_create_iam_role(event_details.clone(), role.role_name, e),
                None,
            )),
        }
    }

    let temp_dir = kubernetes.get_temp_dir(event_details.clone())?;
    let aws_eks_client = match get_rusoto_eks_client(event_details.clone(), kubernetes) {
        Ok(value) => Some(value),
        Err(_) => None,
    };

    let node_groups_with_desired_states = should_update_desired_nodes(
        event_details.clone(),
        kubernetes,
        kubernetes_action,
        node_groups,
        aws_eks_client,
    )?;

    // generate terraform files and copy them into temp dir
    let context = tera_context(kubernetes, aws_zones, &node_groups_with_desired_states, options)?;

    if let Err(e) =
        crate::template::generate_and_copy_all_files_into_dir(template_directory, temp_dir.as_str(), context)
    {
        return Err(EngineError::new_cannot_copy_files_from_one_directory_to_another(
            event_details,
            template_directory.to_string(),
            temp_dir,
            e,
        ));
    }

    // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/aws/bootstrap/common/charts directory.
    // this is due to the required dependencies of lib/aws/bootstrap/*.tf files
    let bootstrap_charts_dir = format!("{}/common/bootstrap/charts", kubernetes.context().lib_root_dir());
    let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());
    if let Err(e) = crate::template::copy_non_template_files(&bootstrap_charts_dir, common_charts_temp_dir.as_str()) {
        return Err(EngineError::new_cannot_copy_files_from_one_directory_to_another(
            event_details,
            bootstrap_charts_dir,
            common_charts_temp_dir,
            e,
        ));
    }

    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe(format!("Deploying {} cluster.", kubernetes.kind())),
    ));

    // terraform deployment dedicated to cloud resources
    if let Err(e) = terraform_init_validate_plan_apply(temp_dir.as_str(), kubernetes.context().is_dry_run_deploy()) {
        return Err(EngineError::new_terraform_error(event_details, e));
    }

    let mut cluster_secrets = ClusterSecretsAws::new_from_cluster_secrets_io(
        ClusterSecretsIoAws::new(
            kubernetes.cloud_provider().access_key_id(),
            kubernetes.region().to_string(),
            kubernetes.cloud_provider().secret_access_key(),
            None,
            None,
            kubernetes.kind(),
            kubernetes.cluster_name(),
            kubernetes_long_id.to_string(),
            options.grafana_admin_user.clone(),
            options.grafana_admin_password.clone(),
            kubernetes.cloud_provider().organization_id().to_string(),
            kubernetes.context().is_test_cluster().to_string(),
        ),
        event_details.clone(),
    )?;

    let kubeconfig_path = match kubernetes.kind() {
        Kind::Eks => {
            let current_kubeconfig_path = kubernetes.get_kubeconfig_file_path()?;
            kubernetes.put_kubeconfig_file_to_object_storage(current_kubeconfig_path.as_str())?;
            current_kubeconfig_path
        }
        Kind::Ec2 => {
            let qovery_terraform_config_file = format!("{}/qovery-tf-config.json", &temp_dir);

            // read config generated after terraform infra bootstrap/update
            let qovery_terraform_config = get_aws_ec2_qovery_terraform_config(qovery_terraform_config_file.as_str())
                .map_err(|e| EngineError::new_terraform_error(event_details.clone(), e))?;

            // send cluster info to vault if info mismatch
            // create vault connection (Vault connectivity should not be on the critical deployment path,
            // if it temporarily fails, just ignore it, data will be pushed on the next sync)
            let vault_conn = match QVaultClient::new(event_details.clone()) {
                Ok(x) => Some(x),
                Err(_) => None,
            };
            if let Some(vault) = vault_conn {
                cluster_secrets.k8s_cluster_endpoint = Some(qovery_terraform_config.aws_ec2_public_hostname.clone());
                // update info without taking care of the kubeconfig because we don't have it yet
                let _ = cluster_secrets.create_or_update_secret(&vault, true, event_details.clone());
            };

            let port = match qovery_terraform_config.kubernetes_port_to_u16() {
                Ok(p) => p,
                Err(e) => {
                    return Err(EngineError::new_terraform_error(
                        event_details,
                        TerraformError::ConfigFileInvalidContent {
                            path: qovery_terraform_config_file,
                            raw_message: e,
                        },
                    ))
                }
            };

            // wait for k3s port to be open
            // retry for 10 min, a reboot will occur after 5 min if nothing happens (see EC2 Terraform user config)
            wait_until_port_is_open(
                &TcpCheckSource::DnsName(qovery_terraform_config.aws_ec2_public_hostname.as_str()),
                port,
                600,
                kubernetes.logger(),
                event_details.clone(),
            )
            .map_err(|_| EngineError::new_k8s_cannot_reach_api(event_details.clone()))?;

            // during an instance replacement, the EC2 host dns will change and will require the kubeconfig to be updated
            // we need to ensure the kubeconfig is the correct one by checking the current instance dns in the kubeconfig
            let result = retry::retry(Fixed::from_millis(5 * 1000).take(120), || {
                // force s3 kubeconfig retrieve
                if let Err(e) = kubernetes.delete_local_kubeconfig_object_storage_folder() {
                    return OperationResult::Err(e);
                };
                let (current_kubeconfig_path, mut kubeconfig_file) = match kubernetes.get_kubeconfig_file() {
                    Ok(x) => x,
                    Err(e) => return OperationResult::Retry(e),
                };

                // ensure the kubeconfig content address match with the current instance dns
                let mut buffer = String::new();
                let _ = kubeconfig_file.read_to_string(&mut buffer);
                match buffer.contains(&qovery_terraform_config.aws_ec2_public_hostname) {
                    true => {
                        kubernetes.logger().log(EngineEvent::Info(
                            event_details.clone(),
                            EventMessage::new_from_safe(format!(
                                "kubeconfig stored on s3 do correspond with the actual host {}",
                                &qovery_terraform_config.aws_ec2_public_hostname
                            )),
                        ));
                        OperationResult::Ok(current_kubeconfig_path)
                    }
                    false => {
                        kubernetes.logger().log(EngineEvent::Warning(
                                event_details.clone(),
                                EventMessage::new_from_safe(format!(
                                    "kubeconfig stored on s3 do not yet correspond with the actual host {}, retrying in 5 sec...",
                                    &qovery_terraform_config.aws_ec2_public_hostname
                                )),
                            ));
                        OperationResult::Retry(EngineError::new_kubeconfig_file_do_not_match_the_current_cluster(
                            event_details.clone(),
                        ))
                    }
                }
            });

            match result {
                Ok(x) => x,
                Err(Operation { error, .. }) => return Err(error),
                Err(Error::Internal(_)) => {
                    return Err(EngineError::new_kubeconfig_file_do_not_match_the_current_cluster(event_details))
                }
            }
        }
        _ => {
            return Err(EngineError::new_unsupported_cluster_kind(
                event_details,
                &kubernetes.kind().to_string(),
                CommandError::new_from_safe_message(format!(
                    "expected AWS provider here, while {} was found",
                    kubernetes.kind()
                )),
            ))
        }
    };

    // kubernetes helm deployments on the cluster
    let kubeconfig_path = Path::new(&kubeconfig_path);

    let credentials_environment_variables: Vec<(String, String)> = kubernetes
        .cloud_provider()
        .credentials_environment_variables()
        .into_iter()
        .map(|x| (x.0.to_string(), x.1.to_string()))
        .collect();

    if let Err(e) = kubectl_are_qovery_infra_pods_executed(kubeconfig_path, &credentials_environment_variables) {
        kubernetes.logger().log(EngineEvent::Warning(
            event_details.clone(),
            EventMessage::new("Didn't manage to restart all paused pods".to_string(), Some(e.to_string())),
        ));
    }

    // send cluster info with kubeconfig
    // create vault connection (Vault connectivity should not be on the critical deployment path,
    // if it temporarily fails, just ignore it, data will be pushed on the next sync)
    let vault_conn = match QVaultClient::new(event_details.clone()) {
        Ok(x) => Some(x),
        Err(_) => None,
    };
    if let Some(vault) = vault_conn {
        // encode base64 kubeconfig
        let kubeconfig_content =
            fs::read_to_string(kubeconfig_path).expect("kubeconfig was not found while it should be present");
        let kubeconfig_b64 = base64::encode(kubeconfig_content);
        cluster_secrets.kubeconfig_b64 = Some(kubeconfig_b64);

        // update info without taking care of the kubeconfig because we don't have it yet
        let _ = cluster_secrets.create_or_update_secret(&vault, false, event_details.clone());
    };

    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Preparing chart configuration to be deployed".to_string()),
    ));

    let helm_charts_to_deploy = match kubernetes.kind() {
        Kind::Eks => {
            let charts_prerequisites = EksChartsConfigPrerequisites {
                organization_id: kubernetes.cloud_provider().organization_id().to_string(),
                organization_long_id: kubernetes.cloud_provider().organization_long_id(),
                infra_options: options.clone(),
                cluster_id: kubernetes.id().to_string(),
                cluster_long_id: kubernetes_long_id,
                region: kubernetes.region().to_string(),
                cluster_name: kubernetes.cluster_name(),
                cloud_provider: "aws".to_string(),
                test_cluster: kubernetes.context().is_test_cluster(),
                aws_access_key_id: kubernetes.cloud_provider().access_key_id(),
                aws_secret_access_key: kubernetes.cloud_provider().secret_access_key(),
                vpc_qovery_network_mode: options.vpc_qovery_network_mode.clone(),
                qovery_engine_location: options.qovery_engine_location.clone(),
                ff_log_history_enabled: kubernetes.context().is_feature_enabled(&Features::LogsHistory),
                ff_metrics_history_enabled: kubernetes.context().is_feature_enabled(&Features::MetricsHistory),
                managed_dns_name: kubernetes.dns_provider().domain().to_string(),
                managed_dns_helm_format: kubernetes.dns_provider().domain().to_helm_format_string(),
                managed_dns_resolvers_terraform_format: managed_dns_resolvers_terraform_format(
                    kubernetes.dns_provider(),
                ),
                external_dns_provider: kubernetes.dns_provider().provider_name().to_string(),
                dns_email_report: options.tls_email_report.clone(),
                acme_url: lets_encrypt_url(kubernetes.context()),
                dns_provider_config: kubernetes.dns_provider().provider_configuration(),
                disable_pleco: kubernetes.context().disable_pleco(),
                cluster_advanced_settings: kubernetes.advanced_settings().clone(),
            };
            eks_aws_helm_charts(
                format!("{}/qovery-tf-config.json", &temp_dir).as_str(),
                &charts_prerequisites,
                Some(&temp_dir),
                kubeconfig_path,
                &credentials_environment_variables,
            )
            .map_err(|e| EngineError::new_helm_charts_setup_error(event_details.clone(), e))?
        }
        Kind::Ec2 => {
            let charts_prerequisites = Ec2ChartsConfigPrerequisites {
                organization_id: kubernetes.cloud_provider().organization_id().to_string(),
                organization_long_id: kubernetes.cloud_provider().organization_long_id(),
                infra_options: options.clone(),
                cluster_id: kubernetes.id().to_string(),
                cluster_long_id: kubernetes_long_id,
                region: kubernetes.region().to_string(),
                cluster_name: kubernetes.cluster_name(),
                cloud_provider: "aws".to_string(),
                test_cluster: kubernetes.context().is_test_cluster(),
                aws_access_key_id: kubernetes.cloud_provider().access_key_id(),
                aws_secret_access_key: kubernetes.cloud_provider().secret_access_key(),
                vpc_qovery_network_mode: options.vpc_qovery_network_mode.clone(),
                qovery_engine_location: options.qovery_engine_location.clone(),
                ff_log_history_enabled: kubernetes.context().is_feature_enabled(&Features::LogsHistory),
                ff_metrics_history_enabled: kubernetes.context().is_feature_enabled(&Features::MetricsHistory),
                managed_dns_name: kubernetes.dns_provider().domain().to_string(),
                managed_dns_name_wildcarded: kubernetes.dns_provider().domain().wildcarded().to_string(),
                managed_dns_helm_format: kubernetes.dns_provider().domain().to_helm_format_string(),
                managed_dns_resolvers_terraform_format: managed_dns_resolvers_terraform_format(
                    kubernetes.dns_provider(),
                ),
                external_dns_provider: kubernetes.dns_provider().provider_name().to_string(),
                dns_email_report: options.tls_email_report.clone(),
                acme_url: lets_encrypt_url(kubernetes.context()),
                dns_provider_config: kubernetes.dns_provider().provider_configuration(),
                disable_pleco: kubernetes.context().disable_pleco(),
            };
            ec2_aws_helm_charts(
                format!("{}/qovery-tf-config.json", &temp_dir).as_str(),
                &charts_prerequisites,
                Some(&temp_dir),
                kubeconfig_path,
                &credentials_environment_variables,
            )
            .map_err(|e| EngineError::new_helm_charts_setup_error(event_details.clone(), e))?
        }
        _ => {
            let safe_message = format!("unsupported requested cluster type: {}", kubernetes.kind());
            return Err(EngineError::new_unsupported_cluster_kind(
                event_details,
                &safe_message,
                CommandError::new(safe_message.to_string(), None, None),
            ));
        }
    };

    deploy_charts_levels(
        kubeconfig_path,
        &credentials_environment_variables,
        helm_charts_to_deploy,
        kubernetes.context().is_dry_run_deploy(),
    )
    .map_err(|e| EngineError::new_helm_charts_deploy_error(event_details.clone(), e))
}

fn create_error(kubernetes: &dyn Kubernetes) -> Result<(), EngineError> {
    let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Create));
    let (kubeconfig_path, _) = kubernetes.get_kubeconfig_file()?;
    let environment_variables = kubernetes.cloud_provider().credentials_environment_variables();

    kubernetes.logger().log(EngineEvent::Warning(
        kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Create)),
        EventMessage::new_from_safe(format!("{}.create_error() called.", kubernetes.kind())),
    ));

    match kubectl_exec_get_events(kubeconfig_path, None, environment_variables) {
        Ok(ok_line) => kubernetes
            .logger()
            .log(EngineEvent::Info(event_details, EventMessage::new(ok_line, None))),
        Err(err) => kubernetes.logger().log(EngineEvent::Warning(
            event_details,
            EventMessage::new(
                "Error trying to get kubernetes events".to_string(),
                Some(err.message(ErrorMessageVerbosity::FullDetails)),
            ),
        )),
    };

    Ok(())
}

fn upgrade_error(kubernetes: &dyn Kubernetes) -> Result<(), EngineError> {
    kubernetes.logger().log(EngineEvent::Warning(
        kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Upgrade)),
        EventMessage::new_from_safe(format!("{}.upgrade_error() called.", kubernetes.kind())),
    ));

    Ok(())
}

fn downgrade() -> Result<(), EngineError> {
    Ok(())
}

fn downgrade_error(kubernetes: &dyn Kubernetes) -> Result<(), EngineError> {
    kubernetes.logger().log(EngineEvent::Warning(
        kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Downgrade)),
        EventMessage::new_from_safe(format!("{}.downgrade_error() called.", kubernetes.kind())),
    ));

    Ok(())
}

fn pause(
    kubernetes: &dyn Kubernetes,
    template_directory: &str,
    aws_zones: &[AwsZones],
    node_groups: &[NodeGroups],
    options: &Options,
) -> Result<(), EngineError> {
    let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Pause));

    kubernetes.logger().log(EngineEvent::Info(
        kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Pause)),
        EventMessage::new_from_safe("Preparing cluster pause.".to_string()),
    ));

    let temp_dir = kubernetes.get_temp_dir(event_details.clone())?;

    let aws_eks_client = match get_rusoto_eks_client(event_details.clone(), kubernetes) {
        Ok(value) => Some(value),
        Err(_) => None,
    };

    let node_groups_with_desired_states = should_update_desired_nodes(
        event_details.clone(),
        kubernetes,
        KubernetesClusterAction::Pause,
        node_groups,
        aws_eks_client,
    )?;

    // generate terraform files and copy them into temp dir
    let mut context = tera_context(kubernetes, aws_zones, &node_groups_with_desired_states, options)?;

    // pause: remove all worker nodes to reduce the bill but keep master to keep all the deployment config, certificates etc...
    let worker_nodes: Vec<NodeGroupsFormat> = Vec::new();
    context.insert("eks_worker_nodes", &worker_nodes);

    if let Err(e) =
        crate::template::generate_and_copy_all_files_into_dir(template_directory, temp_dir.as_str(), context)
    {
        return Err(EngineError::new_cannot_copy_files_from_one_directory_to_another(
            event_details,
            template_directory.to_string(),
            temp_dir,
            e,
        ));
    }

    // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/aws/bootstrap-{type}/common/charts directory.
    // this is due to the required dependencies of lib/aws/bootstrap-{type}/*.tf files
    let bootstrap_charts_dir = format!("{}/common/bootstrap/charts", kubernetes.context().lib_root_dir());
    let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());
    if let Err(e) = crate::template::copy_non_template_files(&bootstrap_charts_dir, common_charts_temp_dir.as_str()) {
        return Err(EngineError::new_cannot_copy_files_from_one_directory_to_another(
            event_details,
            bootstrap_charts_dir,
            common_charts_temp_dir,
            e,
        ));
    }

    // pause: only select terraform workers elements to pause to avoid applying on the whole config
    // this to avoid failures because of helm deployments on removing workers nodes
    let tf_workers_resources = match terraform_init_validate_state_list(temp_dir.as_str()) {
        Ok(x) => {
            let mut tf_workers_resources_name = Vec::new();
            for name in x {
                if name.starts_with("aws_eks_node_group.") {
                    tf_workers_resources_name.push(name);
                }
            }
            tf_workers_resources_name
        }
        Err(e) => {
            let error = EngineError::new_terraform_error(event_details, e);
            kubernetes.logger().log(EngineEvent::Error(error.clone(), None));
            return Err(error);
        }
    };

    if tf_workers_resources.is_empty() {
        kubernetes.logger().log(EngineEvent::Warning(
            event_details,
            EventMessage::new_from_safe(
                "Could not find workers resources in terraform state. Cluster seems already paused.".to_string(),
            ),
        ));
        return Ok(());
    }

    let kubernetes_config_file_path = kubernetes.get_kubeconfig_file_path()?;

    // pause: wait 1h for the engine to have 0 running jobs before pausing and avoid getting unreleased lock (from helm or terraform for example)
    if options.qovery_engine_location == EngineLocation::ClientSide {
        match kubernetes.context().is_feature_enabled(&Features::MetricsHistory) {
            true => {
                let metric_name = "taskmanager_nb_running_tasks";
                let wait_engine_job_finish = retry::retry(Fixed::from_millis(60000).take(60), || {
                    return match kubectl_exec_api_custom_metrics(
                        &kubernetes_config_file_path,
                        kubernetes.cloud_provider().credentials_environment_variables(),
                        "qovery",
                        None,
                        metric_name,
                    ) {
                        Ok(metrics) => {
                            let mut current_engine_jobs = 0;

                            for metric in metrics.items {
                                match metric.value.parse::<i32>() {
                                    Ok(job_count) if job_count > 0 => current_engine_jobs += 1,
                                    Err(e) => {
                                        return OperationResult::Retry(EngineError::new_cannot_get_k8s_api_custom_metrics(
                                            event_details.clone(),
                                            CommandError::new("Error while looking at the API metric value".to_string(), Some(e.to_string()), None)));
                                    }
                                    _ => {}
                                }
                            }

                            if current_engine_jobs == 0 {
                                OperationResult::Ok(())
                            } else {
                                OperationResult::Retry(EngineError::new_cannot_pause_cluster_tasks_are_running(event_details.clone(), None))
                            }
                        }
                        Err(e) => {
                            OperationResult::Retry(
                                EngineError::new_cannot_get_k8s_api_custom_metrics(event_details.clone(), e))
                        }
                    };
                });

                match wait_engine_job_finish {
                    Ok(_) => {
                        kubernetes.logger().log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe("No current running jobs on the Engine, infrastructure pause is allowed to start".to_string())));
                    }
                    Err(Operation { error, .. }) => {
                        return Err(error);
                    }
                    Err(Error::Internal(msg)) => {
                        return Err(EngineError::new_cannot_pause_cluster_tasks_are_running(event_details, Some(CommandError::new_from_safe_message(msg))));
                    }
                }
            }
            false => kubernetes.logger().log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe("Engines are running Client side, but metric history flag is disabled. You will encounter issues during cluster lifecycles if you do not enable metric history".to_string()))),
        }
    }

    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Pausing cluster deployment.".to_string()),
    ));

    match terraform_apply_with_tf_workers_resources(temp_dir.as_str(), tf_workers_resources) {
        Ok(_) => {
            let message = format!("Kubernetes cluster {} successfully paused", kubernetes.name());
            kubernetes
                .logger()
                .log(EngineEvent::Info(event_details, EventMessage::new_from_safe(message)));

            Ok(())
        }
        Err(e) => Err(EngineError::new_terraform_error(event_details, e)),
    }
}

fn pause_error(kubernetes: &dyn Kubernetes) -> Result<(), EngineError> {
    kubernetes.logger().log(EngineEvent::Warning(
        kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Pause)),
        EventMessage::new_from_safe(format!("{}.pause_error() called.", kubernetes.kind())),
    ));

    Ok(())
}

fn delete(
    kubernetes: &dyn Kubernetes,
    template_directory: &str,
    aws_zones: &[AwsZones],
    node_groups: &[NodeGroups],
    options: &Options,
) -> Result<(), EngineError> {
    let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Delete));
    let mut skip_kubernetes_step = false;

    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe(format!("Preparing to delete {} cluster.", kubernetes.kind())),
    ));

    let temp_dir = kubernetes.get_temp_dir(event_details.clone())?;
    let node_groups_with_desired_states = match kubernetes.kind() {
        Kind::Eks => {
            let aws_eks_client = match get_rusoto_eks_client(event_details.clone(), kubernetes) {
                Ok(value) => Some(value),
                Err(_) => None,
            };

            should_update_desired_nodes(
                event_details.clone(),
                kubernetes,
                KubernetesClusterAction::Delete,
                node_groups,
                aws_eks_client,
            )?
        }
        Kind::Ec2 => {
            vec![NodeGroupsWithDesiredState::new_from_node_groups(
                &node_groups[0],
                1,
                false,
            )]
        }
        _ => {
            return Err(EngineError::new_unsupported_cluster_kind(
                event_details,
                "only AWS clusters are supported for this delete method",
                CommandError::new_from_safe_message(
                    "please contact Qovery, deletion can't happen on something else than AWS clsuter type".to_string(),
                ),
            ))
        }
    };

    // delete kubeconfig on s3 to avoid obsolete kubeconfig (not for EC2 because S3 kubeconfig upload is not done the same way)
    if kubernetes.kind() != Kind::Ec2 {
        let _ = kubernetes.ensure_kubeconfig_is_not_in_object_storage();
    };

    // generate terraform files and copy them into temp dir
    let context = tera_context(kubernetes, aws_zones, &node_groups_with_desired_states, options)?;

    if let Err(e) =
        crate::template::generate_and_copy_all_files_into_dir(template_directory, temp_dir.as_str(), context)
    {
        return Err(EngineError::new_cannot_copy_files_from_one_directory_to_another(
            event_details,
            template_directory.to_string(),
            temp_dir,
            e,
        ));
    }

    // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/aws/bootstrap/common/charts directory.
    // this is due to the required dependencies of lib/aws/bootstrap/*.tf files
    let bootstrap_charts_dir = format!("{}/common/bootstrap/charts", kubernetes.context().lib_root_dir());
    let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());
    if let Err(e) = crate::template::copy_non_template_files(&bootstrap_charts_dir, common_charts_temp_dir.as_str()) {
        return Err(EngineError::new_cannot_copy_files_from_one_directory_to_another(
            event_details,
            bootstrap_charts_dir,
            common_charts_temp_dir,
            e,
        ));
    }

    let kubernetes_config_file_path = match kubernetes.get_kubeconfig_file_path() {
        Ok(x) => x,
        Err(e) => {
            let safe_message = "Skipping Kubernetes uninstall because it can't be reached.";
            kubernetes.logger().log(EngineEvent::Warning(
                event_details.clone(),
                EventMessage::new(safe_message.to_string(), Some(e.message(ErrorMessageVerbosity::FullDetails))),
            ));

            skip_kubernetes_step = true;
            "".to_string()
        }
    };

    // should apply before destroy to be sure destroy will compute on all resources
    // don't exit on failure, it can happen if we resume a destroy process
    let message = format!(
        "Ensuring everything is up to date before deleting cluster {}/{}",
        kubernetes.name(),
        kubernetes.id()
    );

    kubernetes
        .logger()
        .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(message)));

    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Running Terraform apply before running a delete.".to_string()),
    ));

    if let Err(e) = terraform_init_validate_plan_apply(temp_dir.as_str(), false) {
        // An issue occurred during the apply before destroy of Terraform, it may be expected if you're resuming a destroy
        kubernetes.logger().log(EngineEvent::Error(
            EngineError::new_terraform_error(event_details.clone(), e),
            None,
        ));
    };

    if !skip_kubernetes_step {
        // should make the diff between all namespaces and qovery managed namespaces
        let message = format!(
            "Deleting all non-Qovery deployed applications and dependencies for cluster {}/{}",
            kubernetes.name(),
            kubernetes.id()
        );

        kubernetes
            .logger()
            .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(message)));

        let all_namespaces = kubectl_exec_get_all_namespaces(
            &kubernetes_config_file_path,
            kubernetes.cloud_provider().credentials_environment_variables(),
        );

        match all_namespaces {
            Ok(namespace_vec) => {
                let namespaces_as_str = namespace_vec.iter().map(std::ops::Deref::deref).collect();
                let namespaces_to_delete = get_firsts_namespaces_to_delete(namespaces_as_str);

                kubernetes.logger().log(EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe("Deleting non Qovery namespaces".to_string()),
                ));

                for namespace_to_delete in namespaces_to_delete.iter() {
                    match cmd::kubectl::kubectl_exec_delete_namespace(
                        &kubernetes_config_file_path,
                        namespace_to_delete,
                        kubernetes.cloud_provider().credentials_environment_variables(),
                    ) {
                        Ok(_) => kubernetes.logger().log(EngineEvent::Info(
                            event_details.clone(),
                            EventMessage::new_from_safe(format!(
                                "Namespace `{}` deleted successfully.",
                                namespace_to_delete
                            )),
                        )),
                        Err(e) => {
                            if !(e.message(ErrorMessageVerbosity::FullDetails).contains("not found")) {
                                kubernetes.logger().log(EngineEvent::Warning(
                                    event_details.clone(),
                                    EventMessage::new_from_safe(format!(
                                        "Can't delete the namespace `{}`",
                                        namespace_to_delete
                                    )),
                                ));
                            }
                        }
                    }
                }
            }
            Err(e) => {
                let message_safe = format!(
                    "Error while getting all namespaces for Kubernetes cluster {}",
                    kubernetes.name_with_id(),
                );
                kubernetes.logger().log(EngineEvent::Warning(
                    event_details.clone(),
                    EventMessage::new(message_safe, Some(e.message(ErrorMessageVerbosity::FullDetails))),
                ));
            }
        }

        let message = format!(
            "Deleting all Qovery deployed elements and associated dependencies for cluster {}/{}",
            kubernetes.name(),
            kubernetes.id()
        );

        kubernetes
            .logger()
            .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(message)));

        // delete custom metrics api to avoid stale namespaces on deletion
        let helm = Helm::new(
            &kubernetes_config_file_path,
            &kubernetes.cloud_provider().credentials_environment_variables(),
        )
        .map_err(|e| to_engine_error(&event_details, e))?;
        let chart = ChartInfo::new_from_release_name("metrics-server", "kube-system");
        helm.uninstall(&chart, &[])
            .map_err(|e| to_engine_error(&event_details, e))?;

        // required to avoid namespace stuck on deletion
        uninstall_cert_manager(
            &kubernetes_config_file_path,
            kubernetes.cloud_provider().credentials_environment_variables(),
            event_details.clone(),
            kubernetes.logger(),
        )?;

        kubernetes.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Deleting Qovery managed helm charts".to_string()),
        ));

        let qovery_namespaces = get_qovery_managed_namespaces();
        for qovery_namespace in qovery_namespaces.iter() {
            let charts_to_delete = helm
                .list_release(Some(qovery_namespace), &[])
                .map_err(|e| to_engine_error(&event_details, e))?;

            for chart in charts_to_delete {
                let chart_info = ChartInfo::new_from_release_name(&chart.name, &chart.namespace);
                match helm.uninstall(&chart_info, &[]) {
                    Ok(_) => kubernetes.logger().log(EngineEvent::Info(
                        event_details.clone(),
                        EventMessage::new_from_safe(format!("Chart `{}` deleted", chart.name)),
                    )),
                    Err(e) => kubernetes.logger().log(EngineEvent::Warning(
                        event_details.clone(),
                        EventMessage::new(format!("Can't delete chart `{}`", &chart.name), Some(e.to_string())),
                    )),
                }
            }
        }

        kubernetes.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Deleting Qovery managed namespaces".to_string()),
        ));

        for qovery_namespace in qovery_namespaces.iter() {
            let deletion = cmd::kubectl::kubectl_exec_delete_namespace(
                &kubernetes_config_file_path,
                qovery_namespace,
                kubernetes.cloud_provider().credentials_environment_variables(),
            );
            match deletion {
                Ok(_) => kubernetes.logger().log(EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe(format!("Namespace {} is fully deleted", qovery_namespace)),
                )),
                Err(e) => {
                    if !(e.message(ErrorMessageVerbosity::FullDetails).contains("not found")) {
                        kubernetes.logger().log(EngineEvent::Warning(
                            event_details.clone(),
                            EventMessage::new_from_safe(format!("Can't delete namespace {}.", qovery_namespace)),
                        ))
                    }
                }
            }
        }

        kubernetes.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Delete all remaining deployed helm applications".to_string()),
        ));

        match helm.list_release(None, &[]) {
            Ok(helm_charts) => {
                for chart in helm_charts {
                    let chart_info = ChartInfo::new_from_release_name(&chart.name, &chart.namespace);
                    match helm.uninstall(&chart_info, &[]) {
                        Ok(_) => kubernetes.logger().log(EngineEvent::Info(
                            event_details.clone(),
                            EventMessage::new_from_safe(format!("Chart `{}` deleted", chart.name)),
                        )),
                        Err(e) => kubernetes.logger().log(EngineEvent::Warning(
                            event_details.clone(),
                            EventMessage::new(format!("Error deleting chart `{}`", chart.name), Some(e.to_string())),
                        )),
                    }
                }
            }
            Err(e) => kubernetes.logger().log(EngineEvent::Warning(
                event_details.clone(),
                EventMessage::new("Unable to get helm list".to_string(), Some(e.to_string())),
            )),
        }
    };

    let message = format!("Deleting Kubernetes cluster {}/{}", kubernetes.name(), kubernetes.id());
    kubernetes
        .logger()
        .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(message)));

    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Running Terraform destroy".to_string()),
    ));

    match cmd::terraform::terraform_init_validate_destroy(temp_dir.as_str(), false) {
        Ok(_) => {
            kubernetes.logger().log(EngineEvent::Info(
                event_details.clone(),
                EventMessage::new_from_safe("Kubernetes cluster successfully deleted".to_string()),
            ));
            Ok(())
        }
        Err(err) => return Err(EngineError::new_terraform_error(event_details, err)),
    }?;

    // delete info on vault
    let vault_conn = QVaultClient::new(event_details);
    if let Ok(vault_conn) = vault_conn {
        let mount = secret_manager::vault::get_vault_mount_name(kubernetes.context().is_test_cluster());

        // ignore on failure
        let _ = vault_conn.delete_secret(mount.as_str(), kubernetes.id());
    };

    Ok(())
}

fn delete_error(kubernetes: &dyn Kubernetes) -> Result<(), EngineError> {
    kubernetes.logger().log(EngineEvent::Warning(
        kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Delete)),
        EventMessage::new_from_safe(format!("{}.delete_error() called.", kubernetes.kind())),
    ));

    Ok(())
}

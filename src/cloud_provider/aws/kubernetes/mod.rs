use core::fmt;
use k8s_openapi::api::apps::v1::DaemonSet;
use kube::api::{Patch, PatchParams};
use kube::Api;

use std::collections::HashSet;
use std::io::Read;
use std::path::Path;
use std::str::FromStr;

use retry::delay::Fixed;
use retry::Error::Operation;
use retry::{Error, OperationResult};
use rusoto_core::credential::StaticProvider;
use rusoto_core::{Client, HttpClient, Region as RusotoRegion};
use rusoto_eks::{DescribeNodegroupRequest, Eks, EksClient, ListNodegroupsRequest, NodegroupScalingConfig};
use serde::{Deserialize, Serialize};
use tera::Context as TeraContext;

use crate::cloud_provider::aws::kubernetes::addons::aws_ebs_csi_addon::AwsEbsCsiAddon;
use crate::cloud_provider::aws::kubernetes::addons::aws_vpc_cni_addon::AwsVpcCniAddon;
use crate::cloud_provider::aws::kubernetes::ec2_helm_charts::{
    ec2_aws_helm_charts, get_aws_ec2_qovery_terraform_config, Ec2ChartsConfigPrerequisites,
};
use crate::cloud_provider::aws::kubernetes::eks_helm_charts::{eks_aws_helm_charts, EksChartsConfigPrerequisites};
use crate::cloud_provider::aws::models::QoveryAwsSdkConfigEc2;
use crate::cloud_provider::aws::regions::{AwsRegion, AwsZones};
use crate::cloud_provider::helm::{deploy_charts_levels, ChartInfo};
use crate::cloud_provider::kubernetes::{
    is_kubernetes_upgrade_required, uninstall_cert_manager, Kind, Kubernetes, ProviderOptions,
};
use crate::cloud_provider::models::{
    CpuArchitecture, KubernetesClusterAction, NodeGroups, NodeGroupsFormat, NodeGroupsWithDesiredState,
};
use crate::cloud_provider::qovery::EngineLocation;
use crate::cloud_provider::utilities::{wait_until_port_is_open, TcpCheckSource};
use crate::cloud_provider::vault::{ClusterSecrets, ClusterSecretsAws};
use crate::cloud_provider::CloudProvider;
use crate::cmd::helm::{to_engine_error, Helm};
use crate::cmd::kubectl::{kubectl_exec_api_custom_metrics, kubectl_exec_get_all_namespaces, kubectl_exec_get_events};
use crate::cmd::kubectl_utils::kubectl_are_qovery_infra_pods_executed;
use crate::cmd::terraform::{
    force_terraform_ec2_instance_type_switch, terraform_apply_with_tf_workers_resources, terraform_import,
    terraform_init_validate_plan_apply, terraform_init_validate_state_list, TerraformError,
};
use crate::deletion_utilities::{get_firsts_namespaces_to_delete, get_qovery_managed_namespaces};
use crate::dns_provider::DnsProvider;
use crate::errors::{CommandError, EngineError, ErrorMessageVerbosity, Tag};
use crate::events::{EngineEvent, EventDetails, EventMessage, InfrastructureStep, Stage};
use crate::io_models::context::{Context, Features};
use crate::models::domain::{ToHelmString, ToTerraformString};
use crate::models::kubernetes::K8sPod;
use crate::models::third_parties::LetsEncryptConfig;

use crate::object_storage::s3::S3;
use crate::runtime::block_on;
use crate::secret_manager::vault::QVaultClient;

use crate::services::kube_client::SelectK8sResourceBy;
use crate::string::terraform_list_format;
use crate::{cmd, secret_manager};
use chrono::Duration as ChronoDuration;
use tokio::time::Duration;

use self::addons::aws_kube_proxy::AwsKubeProxyAddon;
use self::ec2::EC2;
use self::eks::{delete_eks_nodegroups, select_nodegroups_autoscaling_group_behavior, NodeGroupsDeletionType};
use lazy_static::lazy_static;

mod addons;
pub mod ec2;
mod ec2_helm_charts;
pub mod eks;
pub mod eks_helm_charts;
pub mod helm_charts;
pub mod node;

lazy_static! {
    static ref AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION: ChronoDuration = ChronoDuration::hours(1);
    // https://docs.aws.amazon.com/eks/latest/userguide/managed-node-update-behavior.html
    static ref AWS_EKS_MAX_NODE_DRAIN_TIMEOUT_DURATION: ChronoDuration = ChronoDuration::minutes(15);
}

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
        write!(f, "{self:?}")
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
    // Qovery
    pub qovery_api_url: String,
    pub qovery_grpc_url: String,
    #[serde(default)] // TODO: remove default
    pub qovery_engine_url: String,
    pub jwt_token: String,
    pub qovery_engine_location: EngineLocation,
    pub grafana_admin_user: String,
    pub grafana_admin_password: String,
    pub qovery_ssh_key: String,
    #[serde(default)]
    pub user_ssh_keys: Vec<String>,
    // Others
    pub tls_email_report: String,
    #[serde(default)]
    pub user_network_config: Option<UserNetworkConfig>,
    #[serde(default)]
    pub aws_addon_cni_version_override: Option<String>,
    #[serde(default)]
    pub aws_addon_kube_proxy_version_override: Option<String>,
    #[serde(default)]
    pub aws_addon_ebs_csi_version_override: Option<String>,
    #[serde(default)]
    pub ec2_exposed_port: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserNetworkConfig {
    pub documentdb_subnets_zone_a_ids: Vec<String>,
    pub documentdb_subnets_zone_b_ids: Vec<String>,
    pub documentdb_subnets_zone_c_ids: Vec<String>,

    pub elasticache_subnets_zone_a_ids: Vec<String>,
    pub elasticache_subnets_zone_b_ids: Vec<String>,
    pub elasticache_subnets_zone_c_ids: Vec<String>,

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

fn aws_zones(
    zones: Vec<String>,
    region: &AwsRegion,
    event_details: &EventDetails,
) -> Result<Vec<AwsZones>, Box<EngineError>> {
    let mut aws_zones = vec![];

    for zone in zones {
        match AwsZones::from_string(zone.to_string()) {
            Ok(x) => aws_zones.push(x),
            Err(e) => {
                return Err(Box::new(EngineError::new_unsupported_zone(
                    event_details.clone(),
                    region.to_string(),
                    zone,
                    CommandError::new_from_safe_message(e.to_string()),
                )));
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
) -> Result<usize, Box<EngineError>> {
    if subnet_block.len() % 2 == 1 {
        return Err(Box::new(EngineError::new_subnets_count_is_not_even(
            event_details,
            zone_name.to_string(),
            subnet_block.len(),
        )));
    }

    Ok(subnet_block.len() / 2)
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
    eks_upgrade_timeout_in_min: ChronoDuration,
) -> Result<TeraContext, Box<EngineError>> {
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

        context.insert("rds_subnets_zone_a_ids", &user_network_cfg.rds_subnets_zone_a_ids);
        context.insert("rds_subnets_zone_b_ids", &user_network_cfg.rds_subnets_zone_b_ids);
        context.insert("rds_subnets_zone_c_ids", &user_network_cfg.rds_subnets_zone_c_ids);

        context.insert("aws_vpc_eks_id", &user_network_cfg.aws_vpc_eks_id);

        context.insert("eks_subnets_zone_a_ids", &user_network_cfg.eks_subnets_zone_a_ids);
        context.insert("eks_subnets_zone_b_ids", &user_network_cfg.eks_subnets_zone_b_ids);
        context.insert("eks_subnets_zone_c_ids", &user_network_cfg.eks_subnets_zone_c_ids);
    }

    let format_ips =
        |ips: &Vec<String>| -> Vec<String> { ips.iter().map(|ip| format!("\"{ip}\"")).collect::<Vec<_>>() };

    let aws_zones = zones
        .iter()
        .map(|zone| zone.to_terraform_format_string())
        .collect::<Vec<_>>();

    let mut eks_zone_a_subnet_blocks_private = format_ips(&options.eks_zone_a_subnet_blocks);
    let mut eks_zone_b_subnet_blocks_private = format_ips(&options.eks_zone_b_subnet_blocks);
    let mut eks_zone_c_subnet_blocks_private = format_ips(&options.eks_zone_c_subnet_blocks);

    context.insert(
        "aws_enable_vpc_flow_logs",
        &kubernetes.advanced_settings().aws_vpc_enable_flow_logs,
    );
    context.insert(
        "vpc_flow_logs_retention_days",
        &kubernetes.advanced_settings().aws_vpc_flow_logs_retention_days,
    );
    context.insert(
        "s3_flow_logs_bucket_name",
        format!("qovery-vpc-flow-logs-{}", kubernetes.id()).as_str(),
    );

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
            let max_subnet_zone_c = check_odd_subnets(event_details, "c", &ec2_zone_c_subnet_blocks_private)?;

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

    let region_cluster_id = format!("{}-{}", kubernetes.region(), kubernetes.id());
    let vpc_cidr_block = options.vpc_cidr_block.clone();
    let cloudwatch_eks_log_group = format!("/aws/eks/{}/cluster", kubernetes.cluster_name());
    let eks_cidr_subnet = options.eks_cidr_subnet.clone();
    let ec2_cidr_subnet = options.ec2_cidr_subnet.clone();

    let eks_access_cidr_blocks = format_ips(&options.eks_access_cidr_blocks);
    let ec2_access_cidr_blocks = format_ips(&options.ec2_access_cidr_blocks);

    let qovery_api_url = options.qovery_api_url.clone();
    let rds_cidr_subnet = options.rds_cidr_subnet.clone();
    let documentdb_cidr_subnet = options.documentdb_cidr_subnet.clone();
    let elasticache_cidr_subnet = options.elasticache_cidr_subnet.clone();

    // Qovery
    context.insert("organization_id", kubernetes.cloud_provider().organization_id());
    context.insert(
        "organization_long_id",
        &kubernetes.cloud_provider().organization_long_id().to_string(),
    );
    context.insert("qovery_api_url", &qovery_api_url);

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
    context.insert(
        "acme_server_url",
        LetsEncryptConfig::acme_url_for_given_usage(kubernetes.context().is_test_cluster()).as_str(),
    );

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
    context.insert("kubernetes_cluster_long_id", kubernetes.context().cluster_long_id());
    context.insert("eks_region_cluster_id", region_cluster_id.as_str());
    context.insert("eks_worker_nodes", &node_groups);
    context.insert("ec2_zone_a_subnet_blocks_private", &ec2_zone_a_subnet_blocks_private);
    context.insert("ec2_zone_b_subnet_blocks_private", &ec2_zone_b_subnet_blocks_private);
    context.insert("ec2_zone_c_subnet_blocks_private", &ec2_zone_c_subnet_blocks_private);
    context.insert("eks_zone_a_subnet_blocks_private", &eks_zone_a_subnet_blocks_private);
    context.insert("eks_zone_b_subnet_blocks_private", &eks_zone_b_subnet_blocks_private);
    context.insert("eks_zone_c_subnet_blocks_private", &eks_zone_c_subnet_blocks_private);
    context.insert("eks_masters_version", &kubernetes.version().to_string());
    context.insert("eks_workers_version", &kubernetes.version().to_string());
    context.insert("ec2_masters_version", &kubernetes.version().to_string());
    context.insert("ec2_workers_version", &kubernetes.version().to_string());
    context.insert("k3s_version", &kubernetes.version().to_string());

    context.insert("eks_upgrade_timeout_in_min", &eks_upgrade_timeout_in_min.num_minutes());

    // TODO(ENG-1456): remove condition when migration is done
    if let (Some(suffix), Some(patch)) = (kubernetes.version().suffix(), kubernetes.version().patch()) {
        if suffix.as_ref() == "+k3s1" && patch == &8 {
            context.insert("is_old_k3s_version", &true);
            context.insert("ec2_port", &9876.to_string());
        }
    }

    if let Some(port) = options.ec2_exposed_port {
        context.insert("ec2_port", &port.to_string());
    }

    context.insert("cloudwatch_eks_log_group", &cloudwatch_eks_log_group);
    context.insert(
        "aws_cloudwatch_eks_logs_retention_days",
        &kubernetes.advanced_settings().aws_cloudwatch_eks_logs_retention_days,
    );
    context.insert("eks_access_cidr_blocks", &eks_access_cidr_blocks);
    context.insert("ec2_access_cidr_blocks", &ec2_access_cidr_blocks);

    // AWS - EKS/EC2 Metadata
    // https://docs.aws.amazon.com/AWSEC2/latest/UserGuide/configuring-IMDS-existing-instances.html
    context.insert(
        "ec2_metadata_imds_version",
        &kubernetes.advanced_settings().aws_eks_ec2_metadata_imds,
    );

    // AWS - RDS
    context.insert("rds_cidr_subnet", &rds_cidr_subnet);
    context.insert("rds_zone_a_subnet_blocks", &rds_zone_a_subnet_blocks);
    context.insert("rds_zone_b_subnet_blocks", &rds_zone_b_subnet_blocks);
    context.insert("rds_zone_c_subnet_blocks", &rds_zone_c_subnet_blocks);
    context.insert(
        "database_postgresql_deny_public_access",
        &kubernetes.advanced_settings().database_postgresql_deny_public_access,
    );
    context.insert(
        "database_postgresql_allowed_cidrs",
        &format_ips(&kubernetes.advanced_settings().database_postgresql_allowed_cidrs),
    );
    context.insert(
        "database_mysql_deny_public_access",
        &kubernetes.advanced_settings().database_mysql_deny_public_access,
    );
    context.insert(
        "database_mysql_allowed_cidrs",
        &format_ips(&kubernetes.advanced_settings().database_mysql_allowed_cidrs),
    );

    // AWS - DocumentDB
    context.insert("documentdb_cidr_subnet", &documentdb_cidr_subnet);
    context.insert("documentdb_zone_a_subnet_blocks", &documentdb_zone_a_subnet_blocks);
    context.insert("documentdb_zone_b_subnet_blocks", &documentdb_zone_b_subnet_blocks);
    context.insert("documentdb_zone_c_subnet_blocks", &documentdb_zone_c_subnet_blocks);
    context.insert(
        "database_mongodb_deny_public_access",
        &kubernetes.advanced_settings().database_mongodb_deny_public_access,
    );
    context.insert(
        "database_mongodb_allowed_cidrs",
        &format_ips(&kubernetes.advanced_settings().database_mongodb_allowed_cidrs),
    );

    // AWS - Elasticache
    context.insert("elasticache_cidr_subnet", &elasticache_cidr_subnet);
    context.insert("elasticache_zone_a_subnet_blocks", &elasticache_zone_a_subnet_blocks);
    context.insert("elasticache_zone_b_subnet_blocks", &elasticache_zone_b_subnet_blocks);
    context.insert("elasticache_zone_c_subnet_blocks", &elasticache_zone_c_subnet_blocks);
    context.insert(
        "database_redis_deny_public_access",
        &kubernetes.advanced_settings().database_redis_deny_public_access,
    );
    context.insert(
        "database_redis_allowed_cidrs",
        &format_ips(&kubernetes.advanced_settings().database_redis_allowed_cidrs),
    );

    // grafana credentials
    context.insert("grafana_admin_user", options.grafana_admin_user.as_str());
    context.insert("grafana_admin_password", options.grafana_admin_password.as_str());

    // qovery
    context.insert("qovery_api_url", options.qovery_api_url.as_str());
    context.insert("qovery_ssh_key", options.qovery_ssh_key.as_str());
    // AWS support only 1 ssh key
    let user_ssh_key: Option<&str> = options.user_ssh_keys.get(0).map(|x| x.as_str());
    context.insert("user_ssh_key", user_ssh_key.unwrap_or_default());

    // Advanced settings
    context.insert(
        "registry_image_retention_time",
        &kubernetes.advanced_settings().registry_image_retention_time_sec,
    );
    context.insert(
        "resource_expiration_in_seconds",
        &kubernetes.advanced_settings().pleco_resources_ttl,
    );

    // EKS Addons
    if kubernetes.kind() != Kind::Ec2 {
        // CNI
        context.insert(
            "eks_addon_vpc_cni",
            &(match &options.aws_addon_cni_version_override {
                None => AwsVpcCniAddon::new_from_k8s_version(kubernetes.version()),

                Some(overridden_version) => AwsVpcCniAddon::new_with_overridden_version(overridden_version),
            }),
        );
        // Kube-proxy
        context.insert(
            "eks_addon_kube_proxy",
            &(match &options.aws_addon_kube_proxy_version_override {
                None => AwsKubeProxyAddon::new_from_k8s_version(kubernetes.version()),
                Some(overridden_version) => AwsKubeProxyAddon::new_with_overridden_version(overridden_version),
            }),
        );
        // EBS CSI
        context.insert(
            "eks_addon_ebs_csi",
            &(match &options.aws_addon_ebs_csi_version_override {
                None => AwsEbsCsiAddon::new_from_k8s_version(kubernetes.version()),
                Some(overridden_version) => AwsEbsCsiAddon::new_with_overridden_version(overridden_version),
            }),
        );
    }

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
) -> Result<Vec<NodeGroupsWithDesiredState>, Box<EngineError>> {
    let get_autoscaling_config =
        |node_group: &NodeGroups, eks_client: EksClient| -> Result<Option<i32>, Box<EngineError>> {
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
fn get_rusoto_eks_client(
    event_details: EventDetails,
    kubernetes: &dyn Kubernetes,
) -> Result<EksClient, Box<EngineError>> {
    let cloud_provider = kubernetes.cloud_provider();
    let region = match RusotoRegion::from_str(kubernetes.region()) {
        Ok(value) => value,
        Err(error) => {
            return Err(Box::new(EngineError::new_unsupported_region(
                event_details,
                kubernetes.region().to_string(),
                CommandError::new_from_safe_message(error.to_string()),
            )));
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
) -> Result<Option<NodegroupScalingConfig>, Box<EngineError>> {
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
            return Err(Box::new(EngineError::new_nodegroup_list_error(
                event_details,
                CommandError::new(
                    e.to_string(),
                    Some("Error while trying to get node groups from eks".to_string()),
                    None,
                ),
            )))
        }
    };

    // Find eks_node_group that matches the node_group.name passed in parameters
    let mut scaling_config: Option<NodegroupScalingConfig> = None;
    for eks_node_group_name in eks_node_groups {
        // warn: can't filter the state of the autoscaling group with this lib. We should filter on running (and not deleting/creating)
        let eks_node_group = match block_on(eks_client.describe_nodegroup(DescribeNodegroupRequest {
            cluster_name: kubernetes.cluster_name(),
            nodegroup_name: eks_node_group_name.clone(),
        })) {
            Ok(res) => match res.nodegroup {
                None => {
                    return Err(Box::new(EngineError::new_missing_nodegroup_information_error(
                        event_details,
                        eks_node_group_name,
                    )))
                }
                Some(x) => x,
            },
            Err(error) => {
                return Err(Box::new(EngineError::new_cluster_worker_node_not_found(
                    event_details,
                    Some(CommandError::new(
                        "Error while trying to get node groups from AWS".to_string(),
                        Some(error.to_string()),
                        None,
                    )),
                )));
            }
        };
        // ignore if group of nodes is not managed by Qovery
        match eks_node_group.tags {
            None => continue,
            Some(tags) => match tags.get("QoveryNodeGroupName") {
                None => continue,
                Some(tag) => {
                    if tag == &node_group.name {
                        scaling_config = eks_node_group.scaling_config;
                        break;
                    }
                }
            },
        }
    }

    Ok(scaling_config)
}

fn define_cluster_upgrade_timeout(
    pods_list: Vec<K8sPod>,
    kubernetes_action: KubernetesClusterAction,
) -> (ChronoDuration, Option<String>) {
    let mut cluster_upgrade_timeout = *AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION;
    let mut message = None;
    if kubernetes_action != KubernetesClusterAction::Bootstrap {
        // this shouldn't be a blocker in any case
        let mut max_termination_period_found = ChronoDuration::seconds(0);
        let mut pod_names = Vec::new();

        // find the highest termination period among all pods
        for pod in pods_list {
            let current_termination_period = pod
                .metadata
                .termination_grace_period_seconds
                .unwrap_or(ChronoDuration::seconds(0));

            if current_termination_period > max_termination_period_found {
                max_termination_period_found = current_termination_period;
            }

            if current_termination_period > *AWS_EKS_MAX_NODE_DRAIN_TIMEOUT_DURATION {
                pod_names.push(format!(
                    "{} [{:?}] ({} seconds)",
                    pod.metadata.name.clone(),
                    pod.status.phase,
                    current_termination_period
                ));
            }
        }

        // update upgrade timeout if required
        let upgrade_time_in_minutes = ChronoDuration::minutes(max_termination_period_found.num_minutes() * 2);
        if !pod_names.is_empty() {
            cluster_upgrade_timeout = upgrade_time_in_minutes;
            message = Some(format!(
                        "Kubernetes workers timeout will be adjusted to {} minutes, because some pods have a termination period greater than 15 min. Pods:\n{}",
                        cluster_upgrade_timeout.num_minutes(), pod_names.join(", ")
                    ));
        }
    };
    (cluster_upgrade_timeout, message)
}

fn create(
    kubernetes: &dyn Kubernetes,
    kubernetes_long_id: uuid::Uuid,
    template_directory: &str,
    aws_zones: &[AwsZones],
    node_groups: &[NodeGroups],
    options: &Options,
) -> Result<(), Box<EngineError>> {
    let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Create));

    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe(format!("Preparing {} cluster deployment.", kubernetes.kind())),
    ));

    let mut cluster_secrets = ClusterSecrets::new_aws_eks(ClusterSecretsAws::new(
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
        kubernetes.cloud_provider().organization_long_id().to_string(),
        kubernetes.context().is_test_cluster(),
    ));
    let temp_dir = kubernetes.get_temp_dir(event_details.clone())?;
    let qovery_terraform_config_file = format!("{}/qovery-tf-config.json", &temp_dir);

    // old method with rusoto
    let aws_eks_client = match get_rusoto_eks_client(event_details.clone(), kubernetes) {
        Ok(value) => Some(value),
        Err(_) => None,
    };

    // aws connection
    let aws_conn = match kubernetes.cloud_provider().aws_sdk_client() {
        Some(x) => x,
        None => return Err(Box::new(EngineError::new_aws_sdk_cannot_get_client(event_details))),
    };

    let terraform_apply = |kubernetes_action: KubernetesClusterAction| {
        let node_groups_with_desired_states = should_update_desired_nodes(
            event_details.clone(),
            kubernetes,
            kubernetes_action,
            node_groups,
            aws_eks_client.clone(),
        )?;

        // in case error, this should no be a blocking error
        let mut cluster_upgrade_timeout_in_min = *AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION;
        if let Ok(kube_client) = kubernetes.q_kube_client() {
            let pods_list = block_on(kube_client.get_pods(event_details.clone(), None, SelectK8sResourceBy::All))
                .unwrap_or(Vec::with_capacity(0));

            let (timeout, message) = define_cluster_upgrade_timeout(pods_list, KubernetesClusterAction::Upgrade(None));
            cluster_upgrade_timeout_in_min = timeout;

            if let Some(x) = message {
                kubernetes
                    .logger()
                    .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(x)));
            }
        };

        // generate terraform files and copy them into temp dir
        let context = tera_context(
            kubernetes,
            aws_zones,
            &node_groups_with_desired_states,
            options,
            cluster_upgrade_timeout_in_min,
        )?;

        if let Err(e) =
            crate::template::generate_and_copy_all_files_into_dir(template_directory, temp_dir.as_str(), context)
        {
            return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                event_details.clone(),
                template_directory.to_string(),
                temp_dir.clone(),
                e,
            )));
        }

        let dirs_to_be_copied_to = vec![
            // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/aws/bootstrap/common/charts directory.
            // this is due to the required dependencies of lib/aws/bootstrap/*.tf files
            (
                format!("{}/common/bootstrap/charts", kubernetes.context().lib_root_dir()),
                format!("{}/common/charts", temp_dir.as_str()),
            ),
            // copy lib/common/bootstrap/chart_values directory (and sub directory) into the lib/aws/bootstrap/common/chart_values directory.
            (
                format!("{}/common/bootstrap/chart_values", kubernetes.context().lib_root_dir()),
                format!("{}/common/chart_values", temp_dir.as_str()),
            ),
        ];
        for (source_dir, target_dir) in dirs_to_be_copied_to {
            if let Err(e) = crate::template::copy_non_template_files(&source_dir, target_dir.as_str()) {
                return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                    event_details.clone(),
                    source_dir,
                    target_dir,
                    e,
                )));
            }
        }

        kubernetes.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe(format!("Deploying {} cluster.", kubernetes.kind())),
        ));

        let tf_apply_result = retry::retry(Fixed::from_millis(3000).take(1), || {
            match terraform_init_validate_plan_apply(
                temp_dir.as_str(),
                kubernetes.context().is_dry_run_deploy(),
                kubernetes
                    .cloud_provider()
                    .credentials_environment_variables()
                    .as_slice(),
            ) {
                Ok(_) => OperationResult::Ok(()),
                Err(e) => {
                    match &e {
                        TerraformError::S3BucketAlreadyOwnedByYou {
                            bucket_name,
                            terraform_resource_name,
                            ..
                        } => {
                            // Try to import S3 bucket and relaunch Terraform apply
                            kubernetes.logger().log(EngineEvent::Warning(
                            event_details.clone(),
                            EventMessage::new(
                                format!("There was an issue trying to create the S3 bucket `{bucket_name}`, trying to import it."),
                                Some(e.to_string()),
                            ),
                        ));
                            match terraform_import(
                                temp_dir.as_str(),
                                format!("aws_s3_bucket.{terraform_resource_name}").as_str(),
                                bucket_name,
                                kubernetes
                                    .cloud_provider()
                                    .credentials_environment_variables()
                                    .as_slice(),
                            ) {
                                Ok(_) => {
                                    kubernetes.logger().log(EngineEvent::Info(
                                        event_details.clone(),
                                        EventMessage::new_from_safe(format!(
                                            "S3 bucket `{bucket_name}` has been imported properly."
                                        )),
                                    ));

                                    // triggering retry (applying Terraform apply)
                                    OperationResult::Retry(Box::new(EngineError::new_terraform_error(
                                        event_details.clone(),
                                        e.clone(),
                                    )))
                                }
                                Err(e) => OperationResult::Err(Box::new(EngineError::new_terraform_error(
                                    event_details.clone(),
                                    e,
                                ))),
                            }
                        }
                        _ => match kubernetes.kind() {
                            Kind::Eks => {
                                // on EKS, clean possible nodegroup deployment failures because of quota issues
                                // do not exit on this error to avoid masking the real Terraform issue
                                kubernetes.logger().log(EngineEvent::Info(
                                    event_details.clone(),
                                    EventMessage::new_from_safe(
                                        "Ensuring no failed nodegroups are present in the cluster, or delete them if at least one active nodegroup is present".to_string()
                                    ),
                                ));
                                if let Err(e) = block_on(delete_eks_nodegroups(
                                    aws_conn.clone(),
                                    kubernetes.cluster_name(),
                                    kubernetes.context().is_first_cluster_deployment(),
                                    NodeGroupsDeletionType::FailedOnly,
                                    event_details.clone(),
                                )) {
                                    // only return failures if the cluster is not absent, because it can be a VPC quota issue
                                    if e.tag() != &Tag::CannotGetCluster {
                                        return OperationResult::Err(e);
                                    }
                                }

                                OperationResult::Err(Box::new(EngineError::new_terraform_error(
                                    event_details.clone(),
                                    e.clone(),
                                )))
                            }
                            Kind::Ec2 => {
                                if let Err(err) = force_terraform_ec2_instance_type_switch(
                                    temp_dir.as_str(),
                                    e.clone(),
                                    kubernetes.logger(),
                                    &event_details,
                                    kubernetes.context().is_dry_run_deploy(),
                                    kubernetes
                                        .cloud_provider()
                                        .credentials_environment_variables()
                                        .as_slice(),
                                ) {
                                    return OperationResult::Err(Box::new(EngineError::new_terraform_error(
                                        event_details.clone(),
                                        err,
                                    )));
                                }

                                OperationResult::Err(Box::new(EngineError::new_terraform_error(
                                    event_details.clone(),
                                    e.clone(),
                                )))
                            }
                            _ => OperationResult::Err(Box::new(EngineError::new_terraform_error(
                                event_details.clone(),
                                e,
                            ))),
                        },
                    }
                }
            }
        });

        match tf_apply_result {
            Ok(_) => Ok(()),
            Err(Operation { error, .. }) => Err(error),
            Err(Error::Internal(e)) => Err(Box::new(EngineError::new_terraform_error(
                event_details.clone(),
                TerraformError::Unknown {
                    terraform_args: vec![],
                    raw_message: e,
                },
            ))),
        }
    };

    // upgrade cluster instead if required
    if kubernetes.context().is_first_cluster_deployment() {
        // terraform deployment dedicated to cloud resources
        terraform_apply(KubernetesClusterAction::Bootstrap)?;
    } else {
        // on EKS, we need to check if there is no already deployed failed nodegroups to avoid future quota issues
        if kubernetes.kind() == Kind::Eks {
            kubernetes.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe(
                "Ensuring no failed nodegroups are present in the cluster, or delete them if at least one active nodegroup is present".to_string(),
            )));
            if let Err(e) = block_on(delete_eks_nodegroups(
                aws_conn.clone(),
                kubernetes.cluster_name(),
                kubernetes.context().is_first_cluster_deployment(),
                NodeGroupsDeletionType::FailedOnly,
                event_details.clone(),
            )) {
                // only return failures if the cluster is not absent, because it can be a VPC quota issue
                if e.tag() != &Tag::CannotGetCluster {
                    return Err(e);
                }
            }
        };
        match kubernetes.get_kubeconfig_file() {
            Ok((path, _)) => match is_kubernetes_upgrade_required(
                path,
                kubernetes.version(),
                kubernetes.cloud_provider().credentials_environment_variables(),
                event_details.clone(),
                kubernetes.logger(),
            ) {
                Ok(x) =>  {
                    if x.required_upgrade_on.is_some() {
                        // useful for debug purpose: we update here Vault with the name of the instance only because k3s is not ready yet (after upgrade)
                        let res =  kubernetes.upgrade_with_status(x);
                        // push endpoint to Vault for EC2
                        if kubernetes.kind() == Kind::Ec2 {
                            let qovery_terraform_config = get_aws_ec2_qovery_terraform_config(qovery_terraform_config_file.as_str())
                                .map_err(|e| EngineError::new_terraform_error(event_details.clone(), e))?;
                            cluster_secrets.set_k8s_cluster_endpoint(qovery_terraform_config.aws_ec2_public_hostname);
                            let _ = kubernetes.update_vault_config(event_details.clone(), qovery_terraform_config_file.clone(), cluster_secrets.clone(), None);
                        };
                        // return error on upgrade failure
                        res?;
                    } else {
                        kubernetes.logger().log(EngineEvent::Info(
                            event_details.clone(),
                            EventMessage::new_from_safe("Kubernetes cluster upgrade not required".to_string()),
                        ));
                    }
                },
                Err(e) => {
                    // Log a warning, this error is not blocking
                    kubernetes.logger().log(EngineEvent::Warning(
                        event_details.clone(),
                        EventMessage::new("Error detected, upgrade won't occurs, but standard deployment.".to_string(), Some(e.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
                        )),
                    );
                }
            },
            Err(_) => kubernetes.logger().log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe("Kubernetes cluster upgrade not required, config file is not found and cluster have certainly never been deployed before".to_string())))
        };
    }

    // apply to generate tf_qovery_config.json
    terraform_apply(KubernetesClusterAction::Update(None))?;

    let kubeconfig_path = match kubernetes.kind() {
        Kind::Eks => {
            let current_kubeconfig_path = kubernetes.get_kubeconfig_file_path()?;
            kubernetes.put_kubeconfig_file_to_object_storage(current_kubeconfig_path.as_str())?;
            current_kubeconfig_path
        }
        Kind::Ec2 => {
            // wait for EC2 k3S kubeconfig to be ready and valid
            // no need to push it to object storage, it's already done by the EC2 instance itself
            let qovery_terraform_config = get_aws_ec2_qovery_terraform_config(qovery_terraform_config_file.as_str())
                .map_err(|e| EngineError::new_terraform_error(event_details.clone(), e))?;
            EC2::get_and_check_if_kubeconfig_is_valid(kubernetes, event_details.clone(), qovery_terraform_config)?
        }
        _ => {
            return Err(Box::new(EngineError::new_unsupported_cluster_kind(
                event_details,
                &kubernetes.kind().to_string(),
                CommandError::new_from_safe_message(format!(
                    "expected AWS provider here, while {} was found",
                    kubernetes.kind()
                )),
            )))
        }
    };

    // send cluster info with kubeconfig
    // create vault connection (Vault connectivity should not be on the critical deployment path,
    // if it temporarily fails, just ignore it, data will be pushed on the next sync)
    let _ = kubernetes.update_vault_config(
        event_details.clone(),
        qovery_terraform_config_file.clone(),
        cluster_secrets,
        Some(kubeconfig_path.clone()),
    );

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

    // When the user control the network/vpc configuration, we may hit a bug of the in tree aws load balancer controller
    // were if there is a custom dns server set for the VPC, kube-proxy nodes are not correctly configured and load balancer healthcheck are failing
    // The correct fix would be to stop using the k8s in tree lb controller, and use instead the external aws lb controller.
    // But as we don't want to do the migration for all users, we will just patch the kube-proxy configuration on the fly
    // https://aws.amazon.com/premiumsupport/knowledge-center/eks-troubleshoot-unhealthy-targets-nlb/
    // https://github.com/kubernetes/kubernetes/issues/80579
    // https://github.com/kubernetes/cloud-provider-aws/issues/87
    if kubernetes.is_network_managed_by_user() && kubernetes.kind() == Kind::Eks {
        info!("patching kube-proxy configuration to fix k8s in tree load balancer controller bug");
        block_on(patch_kube_proxy_for_aws_user_network(kubernetes.kube_client()?)).map_err(|e| {
            EngineError::new_k8s_node_not_ready(
                event_details.clone(),
                CommandError::new_from_safe_message(format!(
                    "Cannot patch kube proxy for user configured network: {e}"
                )),
            )
        })?;
    }

    // retrieve cluster CPU architectures
    let mut nodegroups_arch_set = HashSet::new();
    for n in node_groups {
        nodegroups_arch_set.insert(n.instance_architecture);
    }
    let cpu_architectures = nodegroups_arch_set.into_iter().collect::<Vec<CpuArchitecture>>();

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
                cpu_architectures,
                cloud_provider: "aws".to_string(),
                test_cluster: kubernetes.context().is_test_cluster(),
                aws_access_key_id: kubernetes.cloud_provider().access_key_id(),
                aws_secret_access_key: kubernetes.cloud_provider().secret_access_key(),
                vpc_qovery_network_mode: options.vpc_qovery_network_mode.clone(),
                qovery_engine_location: options.qovery_engine_location.clone(),
                ff_log_history_enabled: kubernetes.context().is_feature_enabled(&Features::LogsHistory),
                ff_metrics_history_enabled: kubernetes.context().is_feature_enabled(&Features::MetricsHistory),
                ff_grafana_enabled: kubernetes.context().is_feature_enabled(&Features::Grafana),
                managed_dns_name: kubernetes.dns_provider().domain().to_string(),
                managed_dns_helm_format: kubernetes.dns_provider().domain().to_helm_format_string(),
                managed_dns_resolvers_terraform_format: managed_dns_resolvers_terraform_format(
                    kubernetes.dns_provider(),
                ),
                managed_dns_root_domain_helm_format: kubernetes
                    .dns_provider()
                    .domain()
                    .root_domain()
                    .to_helm_format_string(),
                external_dns_provider: kubernetes.dns_provider().provider_name().to_string(),
                lets_encrypt_config: LetsEncryptConfig::new(
                    options.tls_email_report.to_string(),
                    kubernetes.context().is_test_cluster(),
                ),
                dns_provider_config: kubernetes.dns_provider().provider_configuration(),
                disable_pleco: kubernetes.context().disable_pleco(),
                cluster_advanced_settings: kubernetes.advanced_settings().clone(),
            };
            eks_aws_helm_charts(
                qovery_terraform_config_file.clone().as_str(),
                &charts_prerequisites,
                Some(&temp_dir),
                kubeconfig_path,
                &credentials_environment_variables,
                &**kubernetes.context().qovery_api,
                kubernetes.customer_helm_charts_override(),
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
                cpu_architectures: cpu_architectures[0],
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
                managed_dns_root_domain_helm_format: kubernetes
                    .dns_provider()
                    .domain()
                    .root_domain()
                    .to_helm_format_string(),
                external_dns_provider: kubernetes.dns_provider().provider_name().to_string(),
                lets_encrypt_config: LetsEncryptConfig::new(
                    options.tls_email_report.to_string(),
                    kubernetes.context().is_test_cluster(),
                ),
                dns_provider_config: kubernetes.dns_provider().provider_configuration(),
                disable_pleco: kubernetes.context().disable_pleco(),
            };
            ec2_aws_helm_charts(
                qovery_terraform_config_file.as_str(),
                &charts_prerequisites,
                Some(&temp_dir),
                kubeconfig_path,
                &credentials_environment_variables,
                &**kubernetes.context().qovery_api,
                kubernetes.customer_helm_charts_override(),
            )
            .map_err(|e| EngineError::new_helm_charts_setup_error(event_details.clone(), e))?
        }
        _ => {
            let safe_message = format!("unsupported requested cluster type: {}", kubernetes.kind());
            return Err(Box::new(EngineError::new_unsupported_cluster_kind(
                event_details,
                &safe_message,
                CommandError::new(safe_message.to_string(), None, None),
            )));
        }
    };

    if kubernetes.kind() == Kind::Ec2 {
        let kube_client = &kubernetes.kube_client()?;
        let result = retry::retry(Fixed::from(Duration::from_secs(60)).take(5), || {
            match deploy_charts_levels(
                kube_client,
                kubeconfig_path,
                &credentials_environment_variables,
                helm_charts_to_deploy.clone(),
                kubernetes.context().is_dry_run_deploy(),
            ) {
                Ok(_) => OperationResult::Ok(()),
                Err(e) => {
                    kubernetes.logger().log(EngineEvent::Warning(
                        event_details.clone(),
                        EventMessage::new(
                            "Didn't manage to update Helm charts. Retrying...".to_string(),
                            Some(e.to_string()),
                        ),
                    ));
                    OperationResult::Retry(e)
                }
            }
        });
        match result {
            Ok(_) => Ok(()),
            Err(Operation { error, .. }) => Err(error),
            Err(Error::Internal(e)) => Err(CommandError::new(
                "Didn't manage to update Helm charts after 5 min.".to_string(),
                Some(e),
                None,
            )),
        }
        .map_err(|e| Box::new(EngineError::new_helm_charts_deploy_error(event_details.clone(), e)))
    } else {
        return deploy_charts_levels(
            &kubernetes.kube_client()?,
            kubeconfig_path,
            &credentials_environment_variables,
            helm_charts_to_deploy,
            kubernetes.context().is_dry_run_deploy(),
        )
        .map_err(|e| Box::new(EngineError::new_helm_charts_deploy_error(event_details.clone(), e)));
    }
}

fn create_error(kubernetes: &dyn Kubernetes) -> Result<(), Box<EngineError>> {
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

fn upgrade_error(kubernetes: &dyn Kubernetes) -> Result<(), Box<EngineError>> {
    kubernetes.logger().log(EngineEvent::Warning(
        kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Upgrade)),
        EventMessage::new_from_safe(format!("{}.upgrade_error() called.", kubernetes.kind())),
    ));

    Ok(())
}

fn pause(
    kubernetes: &dyn Kubernetes,
    template_directory: &str,
    aws_zones: &[AwsZones],
    node_groups: &[NodeGroups],
    options: &Options,
) -> Result<(), Box<EngineError>> {
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

    // in case error, this should no be a blocking error
    let mut cluster_upgrade_timeout_in_min = *AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION;
    if let Ok(kube_client) = kubernetes.q_kube_client() {
        let pods_list = block_on(kube_client.get_pods(event_details.clone(), None, SelectK8sResourceBy::All))
            .unwrap_or(Vec::with_capacity(0));

        let (timeout, message) = define_cluster_upgrade_timeout(pods_list, KubernetesClusterAction::Pause);
        cluster_upgrade_timeout_in_min = timeout;

        if let Some(x) = message {
            kubernetes
                .logger()
                .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(x)));
        }
    };

    // generate terraform files and copy them into temp dir
    let mut context = tera_context(
        kubernetes,
        aws_zones,
        &node_groups_with_desired_states,
        options,
        cluster_upgrade_timeout_in_min,
    )?;

    // pause: remove all worker nodes to reduce the bill but keep master to keep all the deployment config, certificates etc...
    let worker_nodes: Vec<NodeGroupsFormat> = Vec::new();
    context.insert("eks_worker_nodes", &worker_nodes);

    if let Err(e) =
        crate::template::generate_and_copy_all_files_into_dir(template_directory, temp_dir.as_str(), context)
    {
        return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
            event_details,
            template_directory.to_string(),
            temp_dir,
            e,
        )));
    }

    // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/aws/bootstrap-{type}/common/charts directory.
    // this is due to the required dependencies of lib/aws/bootstrap-{type}/*.tf files
    let bootstrap_charts_dir = format!("{}/common/bootstrap/charts", kubernetes.context().lib_root_dir());
    let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());
    if let Err(e) = crate::template::copy_non_template_files(&bootstrap_charts_dir, common_charts_temp_dir.as_str()) {
        return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
            event_details,
            bootstrap_charts_dir,
            common_charts_temp_dir,
            e,
        )));
    }

    // pause: only select terraform workers elements to pause to avoid applying on the whole config
    // this to avoid failures because of helm deployments on removing workers nodes
    let tf_workers_resources = match terraform_init_validate_state_list(
        temp_dir.as_str(),
        kubernetes
            .cloud_provider()
            .credentials_environment_variables()
            .as_slice(),
    ) {
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
            return Err(Box::new(error));
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
                        return Err(Box::new(error));
                    }
                    Err(Error::Internal(msg)) => {
                        return Err(Box::new(EngineError::new_cannot_pause_cluster_tasks_are_running(event_details, Some(CommandError::new_from_safe_message(msg)))));
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

    match terraform_apply_with_tf_workers_resources(
        temp_dir.as_str(),
        tf_workers_resources,
        kubernetes
            .cloud_provider()
            .credentials_environment_variables()
            .as_slice(),
    ) {
        Ok(_) => {
            let message = format!("Kubernetes cluster {} successfully paused", kubernetes.name());
            kubernetes
                .logger()
                .log(EngineEvent::Info(event_details, EventMessage::new_from_safe(message)));

            Ok(())
        }
        Err(e) => Err(Box::new(EngineError::new_terraform_error(event_details, e))),
    }
}

fn pause_error(kubernetes: &dyn Kubernetes) -> Result<(), Box<EngineError>> {
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
) -> Result<(), Box<EngineError>> {
    let event_details = kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Delete));
    let mut skip_kubernetes_step = false;

    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe(format!("Preparing to delete {} cluster.", kubernetes.kind())),
    ));

    let aws_conn = match kubernetes.cloud_provider().aws_sdk_client() {
        Some(x) => x,
        None => return Err(Box::new(EngineError::new_aws_sdk_cannot_get_client(event_details))),
    };

    let temp_dir = kubernetes.get_temp_dir(event_details.clone())?;
    let qovery_terraform_config_file = format!("{}/qovery-tf-config.json", &temp_dir);
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
            return Err(Box::new(EngineError::new_unsupported_cluster_kind(
                event_details,
                "only AWS clusters are supported for this delete method",
                CommandError::new_from_safe_message(
                    "please contact Qovery, deletion can't happen on something else than AWS clsuter type".to_string(),
                ),
            )))
        }
    };

    // generate terraform files and copy them into temp dir
    // in case error, this should no be a blocking error
    let mut cluster_upgrade_timeout_in_min = *AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION;
    if let Ok(kube_client) = kubernetes.q_kube_client() {
        let pods_list = block_on(kube_client.get_pods(event_details.clone(), None, SelectK8sResourceBy::All))
            .unwrap_or(Vec::with_capacity(0));

        let (timeout, message) = define_cluster_upgrade_timeout(pods_list, KubernetesClusterAction::Delete);
        cluster_upgrade_timeout_in_min = timeout;

        if let Some(x) = message {
            kubernetes
                .logger()
                .log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(x)));
        }
    };
    let mut context = tera_context(
        kubernetes,
        aws_zones,
        &node_groups_with_desired_states,
        options,
        cluster_upgrade_timeout_in_min,
    )?;
    context.insert("is_deletion_step", &true);

    if let Err(e) =
        crate::template::generate_and_copy_all_files_into_dir(template_directory, temp_dir.as_str(), context)
    {
        return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
            event_details,
            template_directory.to_string(),
            temp_dir,
            e,
        )));
    }

    // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/aws/bootstrap/common/charts directory.
    // this is due to the required dependencies of lib/aws/bootstrap/*.tf files
    let bootstrap_charts_dir = format!("{}/common/bootstrap/charts", kubernetes.context().lib_root_dir());
    let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());
    if let Err(e) = crate::template::copy_non_template_files(&bootstrap_charts_dir, common_charts_temp_dir.as_str()) {
        return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
            event_details,
            bootstrap_charts_dir,
            common_charts_temp_dir,
            e,
        )));
    }

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

    if let Err(e) = terraform_init_validate_plan_apply(
        temp_dir.as_str(),
        false,
        kubernetes
            .cloud_provider()
            .credentials_environment_variables()
            .as_slice(),
    ) {
        // An issue occurred during the apply before destroy of Terraform, it may be expected if you're resuming a destroy
        kubernetes.logger().log(EngineEvent::Warning(
            event_details.clone(),
            EventMessage::new(
                "Terraform apply before delete failed. It may occur but may not be blocking.".to_string(),
                Some(e.to_string()),
            ),
        ));
    };

    // // delete kubeconfig on s3 to avoid obsolete kubeconfig (not for EC2 because S3 kubeconfig upload is not done the same way)
    if kubernetes.kind() != Kind::Ec2 {
        let _ = kubernetes.ensure_kubeconfig_is_not_in_object_storage();
    };

    let kubernetes_config_file_path = match kubernetes.kind() {
        Kind::Eks => match kubernetes.get_kubeconfig_file_path() {
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
        },
        Kind::Ec2 => {
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
            let mut cluster_secrets = ClusterSecrets::new_aws_eks(ClusterSecretsAws::new(
                kubernetes.cloud_provider().access_key_id(),
                kubernetes.region().to_string(),
                kubernetes.cloud_provider().secret_access_key(),
                None,
                None,
                kubernetes.kind(),
                kubernetes.cluster_name(),
                kubernetes.long_id().to_string(),
                options.grafana_admin_user.clone(),
                options.grafana_admin_password.clone(),
                kubernetes.cloud_provider().organization_id().to_string(),
                kubernetes.context().is_test_cluster(),
            ));
            if let Some(vault) = vault_conn {
                cluster_secrets.set_k8s_cluster_endpoint(qovery_terraform_config.aws_ec2_public_hostname.clone());
                // update info without taking care of the kubeconfig because we don't have it yet
                let _ = cluster_secrets.create_or_update_secret(&vault, true, event_details.clone());
            };

            let port = match qovery_terraform_config.kubernetes_port_to_u16() {
                Ok(p) => p,
                Err(e) => {
                    return Err(Box::new(EngineError::new_terraform_error(
                        event_details,
                        TerraformError::ConfigFileInvalidContent {
                            path: qovery_terraform_config_file,
                            raw_message: e,
                        },
                    )))
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
                        OperationResult::Retry(Box::new(
                            EngineError::new_kubeconfig_file_do_not_match_the_current_cluster(event_details.clone()),
                        ))
                    }
                }
            });

            match result {
                Ok(x) => x,
                Err(Operation { error, .. }) => return Err(error),
                Err(Error::Internal(_)) => {
                    return Err(Box::new(EngineError::new_kubeconfig_file_do_not_match_the_current_cluster(
                        event_details,
                    )))
                }
            }
        }
        _ => {
            let safe_message = "Skipping Kubernetes uninstall because it can't be reached.";
            kubernetes.logger().log(EngineEvent::Warning(
                event_details.clone(),
                EventMessage::new_from_safe(safe_message.to_string()),
            ));
            skip_kubernetes_step = true;
            "".to_string()
        }
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
                                "Namespace `{namespace_to_delete}` deleted successfully."
                            )),
                        )),
                        Err(e) => {
                            if !(e.message(ErrorMessageVerbosity::FullDetails).contains("not found")) {
                                kubernetes.logger().log(EngineEvent::Warning(
                                    event_details.clone(),
                                    EventMessage::new_from_safe(format!(
                                        "Can't delete the namespace `{namespace_to_delete}`"
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
        if let Err(e) = helm.uninstall(&chart, &[]) {
            // this error is not blocking
            kubernetes.logger().log(EngineEvent::Warning(
                event_details.clone(),
                EventMessage::new_from_engine_error(to_engine_error(&event_details, e)),
            ));
        }

        // required to avoid namespace stuck on deletion
        if let Err(e) = uninstall_cert_manager(
            &kubernetes_config_file_path,
            kubernetes.cloud_provider().credentials_environment_variables(),
            event_details.clone(),
            kubernetes.logger(),
        ) {
            // this error is not blocking, logging a warning and move on
            kubernetes.logger().log(EngineEvent::Warning(
                event_details.clone(),
                EventMessage::new(
                    "An error occurred while trying to uninstall cert-manager. This is not blocking.".to_string(),
                    Some(e.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
                ),
            ));
        }

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
                    EventMessage::new_from_safe(format!("Namespace {qovery_namespace} is fully deleted")),
                )),
                Err(e) => {
                    if !(e.message(ErrorMessageVerbosity::FullDetails).contains("not found")) {
                        kubernetes.logger().log(EngineEvent::Warning(
                            event_details.clone(),
                            EventMessage::new_from_safe(format!("Can't delete namespace {qovery_namespace}.")),
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

    if kubernetes.kind() != Kind::Ec2 {
        // remove all node groups to avoid issues because of nodegroups manually added by user, making terraform unable to delete the EKS cluster
        block_on(delete_eks_nodegroups(
            aws_conn,
            kubernetes.cluster_name(),
            kubernetes.context().is_first_cluster_deployment(),
            NodeGroupsDeletionType::All,
            event_details.clone(),
        ))?;

        // remove S3 buckets from tf state
        kubernetes.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Removing S3 logs bucket from tf state".to_string()),
        ));
        let resources_to_be_removed_from_tf_state: Vec<(&str, &str)> = vec![
            ("aws_s3_bucket.loki_bucket", "S3 logs bucket"),
            ("aws_s3_bucket_lifecycle_configuration.loki_lifecycle", "S3 logs lifecycle"),
            ("aws_s3_bucket.vpc_flow_logs", "S3 flow logs bucket"),
            (
                "aws_s3_bucket_lifecycle_configuration.vpc_flow_logs_lifecycle",
                "S3 vpc log flow lifecycle",
            ),
        ];

        for resource_to_be_removed_from_tf_state in resources_to_be_removed_from_tf_state {
            match cmd::terraform::terraform_remove_resource_from_tf_state(
                temp_dir.as_str(),
                resource_to_be_removed_from_tf_state.0,
            ) {
                Ok(_) => {
                    kubernetes.logger().log(EngineEvent::Info(
                        event_details.clone(),
                        EventMessage::new_from_safe(format!(
                            "{} successfully removed from tf state.",
                            resource_to_be_removed_from_tf_state.1
                        )),
                    ));
                }
                Err(err) => {
                    // We weren't able to remove S3 bucket from tf state, maybe it's not there?
                    // Anyways, this is not blocking
                    kubernetes.logger().log(EngineEvent::Warning(
                        event_details.clone(),
                        EventMessage::new_from_engine_error(EngineError::new_terraform_error(
                            event_details.clone(),
                            err,
                        )),
                    ));
                }
            }
        }
    }

    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Running Terraform destroy".to_string()),
    ));

    if kubernetes.kind() == Kind::Ec2 {
        match kubernetes.cloud_provider().aws_sdk_client() {
            None => return Err(Box::new(EngineError::new_aws_sdk_cannot_get_client(event_details))),
            Some(client) => block_on(client.detach_ec2_volumes(kubernetes.id(), &event_details))?,
        };
    }

    if let Err(err) = cmd::terraform::terraform_init_validate_destroy(
        temp_dir.as_str(),
        false,
        kubernetes
            .cloud_provider()
            .credentials_environment_variables()
            .as_slice(),
    ) {
        return Err(Box::new(EngineError::new_terraform_error(event_details, err)));
    }
    kubernetes.logger().log(EngineEvent::Info(
        event_details.clone(),
        EventMessage::new_from_safe("Kubernetes cluster successfully deleted".to_string()),
    ));

    // delete info on vault
    let vault_conn = QVaultClient::new(event_details);
    if let Ok(vault_conn) = vault_conn {
        let mount = secret_manager::vault::get_vault_mount_name(kubernetes.context().is_test_cluster());

        // ignore on failure
        let _ = vault_conn.delete_secret(mount.as_str(), kubernetes.id());
    };

    Ok(())
}

fn delete_error(kubernetes: &dyn Kubernetes) -> Result<(), Box<EngineError>> {
    kubernetes.logger().log(EngineEvent::Warning(
        kubernetes.get_event_details(Stage::Infrastructure(InfrastructureStep::Delete)),
        EventMessage::new_from_safe(format!("{}.delete_error() called.", kubernetes.kind())),
    ));

    Ok(())
}

async fn patch_kube_proxy_for_aws_user_network(kube_client: kube::Client) -> Result<DaemonSet, kube::Error> {
    let daemon_set: Api<DaemonSet> = Api::namespaced(kube_client, "kube-system");
    let patch_params = PatchParams::default();
    let daemonset_patch = serde_json::json!({
      "spec": {
        "template": {
          "spec": {
            "containers": [
              {
                "name": "kube-proxy",
                "command": [
                  "kube-proxy",
                  "--v=2",
                  "--hostname-override=$(NODE_NAME)",
                  "--config=/var/lib/kube-proxy-config/config"
                ],
                "env": [
                  {
                    "name": "NODE_NAME",
                    "valueFrom": {
                      "fieldRef": {
                        "apiVersion": "v1",
                        "fieldPath": "spec.nodeName"
                      }
                    }
                  }
                ]
              }
            ]
          }
        }
      }
    });

    daemon_set
        .patch("kube-proxy", &patch_params, &Patch::Strategic(daemonset_patch))
        .await
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use crate::{
        cloud_provider::{
            aws::kubernetes::{patch_kube_proxy_for_aws_user_network, AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION},
            models::KubernetesClusterAction,
        },
        models::kubernetes::{K8sMetadata, K8sPod, K8sPodPhase, K8sPodStatus},
    };

    use super::define_cluster_upgrade_timeout;

    #[ignore]
    #[tokio::test]
    async fn test_kube_proxy_patch() -> Result<(), Box<dyn std::error::Error>> {
        let kube_client = kube::Client::try_default().await.unwrap();
        patch_kube_proxy_for_aws_user_network(kube_client).await?;

        Ok(())
    }

    #[test]
    fn test_upgrade_timeout() {
        // bootrap
        assert_eq!(
            define_cluster_upgrade_timeout(Vec::new(), KubernetesClusterAction::Bootstrap).0,
            *AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION
        );
        // update without nodes
        assert_eq!(
            define_cluster_upgrade_timeout(Vec::new(), KubernetesClusterAction::Update(None)).0,
            *AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION
        );
        // update with 1 node above termination_grace_period_seconds
        let res = define_cluster_upgrade_timeout(
            vec![
                K8sPod {
                    metadata: K8sMetadata {
                        name: "x".to_string(),
                        namespace: "x".to_string(),
                        termination_grace_period_seconds: Some(Duration::seconds(40)),
                    },
                    status: K8sPodStatus {
                        phase: K8sPodPhase::Running,
                    },
                },
                K8sPod {
                    metadata: K8sMetadata {
                        name: "y".to_string(),
                        namespace: "z".to_string(),
                        termination_grace_period_seconds: Some(Duration::minutes(80)),
                    },
                    status: K8sPodStatus {
                        phase: K8sPodPhase::Pending,
                    },
                },
            ],
            KubernetesClusterAction::Update(None),
        );
        assert_eq!(res.0, Duration::minutes(160));
        assert!(res.1.is_some());
        assert!(res.1.unwrap().contains("160 minutes"));
    }
}

pub mod eks;
pub mod node;

use crate::errors::{CommandError, EngineError};
use crate::events::EventDetails;
use crate::infrastructure::models::cloud_provider::aws::regions::{AwsRegion, AwsZone};
use crate::infrastructure::models::kubernetes::ProviderOptions;
use crate::infrastructure::models::kubernetes::karpenter::KarpenterParameters;
use crate::io_models::engine_location::EngineLocation;
use crate::io_models::metrics::MetricsParameters;
use crate::io_models::models::{VpcCustomRoutingTable, VpcQoveryNetworkMode};
use serde::{Deserialize, Serialize};
// https://docs.aws.amazon.com/eks/latest/userguide/external-snat.html

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
    #[serde(default)] // TODO: remove default
    pub fargate_profile_zone_a_subnet_blocks: Vec<String>,
    #[serde(default)] // TODO: remove default
    pub fargate_profile_zone_b_subnet_blocks: Vec<String>,
    #[serde(default)] // TODO: remove default
    pub fargate_profile_zone_c_subnet_blocks: Vec<String>,
    #[serde(default)] // TODO: remove default
    pub eks_zone_a_nat_gw_for_fargate_subnet_blocks_public: Vec<String>,
    pub vpc_qovery_network_mode: VpcQoveryNetworkMode,
    pub vpc_cidr_block: String,
    pub eks_cidr_subnet: String,
    #[serde(default)] // TODO: remove default
    pub ec2_cidr_subnet: String,
    pub vpc_custom_routing_table: Vec<VpcCustomRoutingTable>,
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
    pub user_provided_network: Option<UserNetworkConfig>,
    #[serde(default)]
    pub aws_addon_cni_version_override: Option<String>,
    #[serde(default)]
    pub aws_addon_kube_proxy_version_override: Option<String>,
    #[serde(default)]
    pub aws_addon_ebs_csi_version_override: Option<String>,
    #[serde(default)]
    pub aws_addon_coredns_version_override: Option<String>,
    #[serde(default)]
    pub ec2_exposed_port: Option<u16>,
    #[serde(default)]
    pub karpenter_parameters: Option<KarpenterParameters>,
    #[serde(default)]
    pub metrics_parameters: Option<MetricsParameters>,
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

    // karpenter
    pub eks_karpenter_fargate_subnets_zone_a_ids: Vec<String>,
    pub eks_karpenter_fargate_subnets_zone_b_ids: Vec<String>,
    pub eks_karpenter_fargate_subnets_zone_c_ids: Vec<String>,
}

impl ProviderOptions for Options {}

fn aws_zones(
    zones: Vec<String>,
    region: &AwsRegion,
    event_details: &EventDetails,
) -> Result<Vec<AwsZone>, Box<EngineError>> {
    let mut aws_zones = vec![];

    for zone in zones {
        match AwsZone::from_string(zone.to_string()) {
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

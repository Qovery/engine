pub mod eks;
pub mod node;

use crate::environment::models::ToCloudProviderFormat;
use crate::errors::{CommandError, EngineError};
use crate::events::EventDetails;
use crate::infrastructure::models::cloud_provider::aws::regions::{AwsRegion, AwsZone};
use crate::infrastructure::models::kubernetes::ProviderOptions;
use crate::infrastructure::models::kubernetes::karpenter::KarpenterParameters;
use crate::infrastructure::models::kubernetes::keda::KedaParameters;
use crate::io_models::database::DiskIOPS;
use crate::io_models::engine_location::EngineLocation;
use crate::io_models::metrics::MetricsParameters;
use crate::io_models::models::{StorageClass, VpcCustomRoutingTable, VpcQoveryNetworkMode};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
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
    pub aws_addon_pod_identity_version_override: Option<String>,
    #[serde(default)]
    pub ec2_exposed_port: Option<u16>,
    #[serde(default)]
    pub karpenter_parameters: Option<KarpenterParameters>,
    #[serde(default)]
    pub keda_parameters: Option<KedaParameters>,
    #[serde(default)]
    pub metrics_parameters: Option<MetricsParameters>,
    #[serde(default)]
    pub resource_tags: std::collections::HashMap<String, String>,
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
    pub eks_private_subnets_zone_a_ids: Vec<String>,
    pub eks_private_subnets_zone_b_ids: Vec<String>,
    pub eks_private_subnets_zone_c_ids: Vec<String>,
    pub eks_create_nodes_in_private_subnet: bool,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AwsStorageType {
    GP2,
    GP3,
    // GP3 { disk_iops: DiskIOPS }, <= Not supported yet, but to be added in the future including IOPS
}

impl ToCloudProviderFormat for AwsStorageType {
    fn to_cloud_provider_format(&self) -> &str {
        match self {
            AwsStorageType::GP2 => "gp2",
            AwsStorageType::GP3 => "gp3",
        }
    }
}

impl Display for AwsStorageType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            AwsStorageType::GP2 => write!(f, "GP2"),
            AwsStorageType::GP3 => write!(f, "GP3"),
        }
    }
}

impl TryFrom<StorageClass> for AwsStorageType {
    type Error = String;

    fn try_from(value: StorageClass) -> Result<Self, Self::Error> {
        match value.to_string().as_str() {
            "aws-ebs-gp2-0" => Ok(AwsStorageType::GP2),
            "aws-ebs-gp3-0" => Ok(AwsStorageType::GP3),
            _ => Err(format!("Unsupported AWS storage class: {value}")),
        }
    }
}

impl AwsStorageType {
    pub fn to_k8s_storage_class(&self) -> String {
        match self {
            AwsStorageType::GP2 => "aws-ebs-gp2-0",
            AwsStorageType::GP3 => "aws-ebs-gp3-0",
        }
        .to_string()
    }

    pub fn get_disk_iops(&self) -> DiskIOPS {
        match self {
            AwsStorageType::GP2 => DiskIOPS::Default,
            AwsStorageType::GP3 => DiskIOPS::Default,
            // AwsStorageType::GP3 { disk_iops } => *disk_iops, <= Not supported yet, but to be added in the future including IOPS
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aws_storage_type_to_cloud_provider_format() {
        assert_eq!(AwsStorageType::GP2.to_cloud_provider_format(), "gp2");
        assert_eq!(AwsStorageType::GP3.to_cloud_provider_format(), "gp3");
    }

    #[test]
    fn test_aws_storage_type_display_fmt() {
        assert_eq!(format!("{}", AwsStorageType::GP2), "GP2");
        assert_eq!(format!("{}", AwsStorageType::GP3), "GP3");
    }

    #[test]
    fn test_aws_storage_type_to_k8s_storage_class() {
        assert_eq!(AwsStorageType::GP2.to_k8s_storage_class(), "aws-ebs-gp2-0");
        assert_eq!(AwsStorageType::GP3.to_k8s_storage_class(), "aws-ebs-gp3-0");
    }

    #[test]
    fn test_aws_storage_type_get_disk_iops() {
        assert_eq!(AwsStorageType::GP2.get_disk_iops(), DiskIOPS::Default);
        assert_eq!(AwsStorageType::GP3.get_disk_iops(), DiskIOPS::Default);
    }

    #[test]
    fn test_aws_storage_type_try_from_storage_class() {
        let gp2 = StorageClass("aws-ebs-gp2-0".to_string());
        let gp3 = StorageClass("aws-ebs-gp3-0".to_string());
        let unknown = StorageClass("unknown-storage-class".to_string());

        assert_eq!(AwsStorageType::try_from(gp2).unwrap(), AwsStorageType::GP2);
        assert_eq!(AwsStorageType::try_from(gp3).unwrap(), AwsStorageType::GP3);
        assert!(AwsStorageType::try_from(unknown).is_err());
    }
}

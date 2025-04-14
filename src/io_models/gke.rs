use crate::infrastructure::models::kubernetes::gcp::{GkeOptions as GkeOptionsModel, VpcMode as GkeVpcMode};
use crate::io_models::engine_location::EngineLocation;
use crate::io_models::metrics::MetricsParameters;
use crate::io_models::models::VpcQoveryNetworkMode;
use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use time::Time;
use time::macros::format_description;

#[derive(Clone, Serialize, Deserialize)]
pub struct UserProvidedVPCNetwork {
    #[serde(skip_serializing_if = "Option::is_none")]
    vpc_project_id: Option<String>,
    vpc_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    subnetwork_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ip_range_pods_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    additional_ip_range_pods_names: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ip_range_services_name: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct GkeOptions {
    // Qovery
    pub qovery_api_url: String,
    pub qovery_grpc_url: String,
    #[serde(default)]
    pub qovery_engine_url: String,
    pub jwt_token: String,
    pub qovery_ssh_key: String,
    #[serde(default)]
    pub user_ssh_keys: Vec<String>,
    pub grafana_admin_user: String,
    pub grafana_admin_password: String,
    pub qovery_engine_location: EngineLocation,

    // Network
    // VPC
    #[serde(default)]
    pub cluster_ipv4_cidr_block: Option<String>,
    #[serde(default)]
    pub services_ipv4_cidr_block: Option<String>,
    #[serde(default)]
    pub user_provided_network: Option<UserProvidedVPCNetwork>,
    #[serde(default)]
    pub vpc_qovery_network_mode: Option<VpcQoveryNetworkMode>,

    // GCP to be checked during integration if needed:
    pub cluster_maintenance_start_time: String,
    #[serde(default)]
    pub cluster_maintenance_end_time: Option<String>,

    // Other
    pub tls_email_report: String,
    pub metrics_parameters: Option<MetricsParameters>,
}

impl GkeOptions {
    fn to_gke_vpc_mode(&self) -> Result<GkeVpcMode, String> {
        let vpc_mode: GkeVpcMode = match &self.user_provided_network {
            None => GkeVpcMode::new_automatic(
                match &self.cluster_ipv4_cidr_block {
                    Some(cidr) => Some(
                        IpNet::from_str(cidr.as_str())
                            .map_err(|e| format!("cannot parse cluster_ipv4_cidr_block to IP Net: `{e}`"))?,
                    ),
                    None => None,
                },
                match &self.services_ipv4_cidr_block {
                    Some(cidr) => Some(
                        IpNet::from_str(cidr.as_str())
                            .map_err(|e| format!("cannot parse services_ipv4_cidr_block to IP Net: `{e}`"))?,
                    ),
                    None => None,
                },
            ),
            Some(user_provided_network) => GkeVpcMode::new_user_network_config(
                user_provided_network.vpc_project_id.clone(),
                user_provided_network.vpc_name.to_string(),
                user_provided_network.subnetwork_name.clone(),
                user_provided_network.ip_range_pods_name.clone(),
                user_provided_network.additional_ip_range_pods_names.clone(),
                user_provided_network.ip_range_services_name.clone(),
            ),
        };

        Ok(vpc_mode)
    }
}

impl TryFrom<GkeOptions> for GkeOptionsModel {
    type Error = String;

    fn try_from(value: GkeOptions) -> Result<GkeOptionsModel, Self::Error> {
        let vpc_mode = value
            .to_gke_vpc_mode()
            .map_err(|e| format!("cannot parse VPCMode: `{e}`"))?;

        Ok(GkeOptionsModel::new(
            value.qovery_api_url,
            value.qovery_grpc_url,
            value.qovery_engine_url,
            value.jwt_token,
            value.qovery_ssh_key,
            value.user_ssh_keys,
            value.grafana_admin_user,
            value.grafana_admin_password,
            value.qovery_engine_location,
            vpc_mode,
            value.vpc_qovery_network_mode,
            value.tls_email_report,
            Time::parse(
                value.cluster_maintenance_start_time.as_str(),
                format_description!("[hour]:[minute]Z"),
            )
            .map_err(|_e| "Cannot parse cluster_maintenance_start_time")?,
            match value.cluster_maintenance_end_time {
                None => None,
                Some(t) => Some(
                    Time::parse(t.as_str(), format_description!("[hour]:[minute]Z"))
                        .map_err(|_e| "Cannot parse cluster_maintenance_end_time")?,
                ),
            },
            value.metrics_parameters,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io_models::engine_location::EngineLocation;
    use ipnet::IpNet;
    use std::str::FromStr;

    #[test]
    fn test_gke_options_to_gke_vpc_mode() {
        // setup:
        let basic_gke_options = GkeOptions {
            qovery_api_url: "https://api.qovery.com".to_string(),
            qovery_grpc_url: "https://grpc.qovery.com".to_string(),
            qovery_engine_url: "https://engine.qovery.com".to_string(),
            jwt_token: "jwt_token".to_string(),
            qovery_ssh_key: "qovery_ssh_key".to_string(),
            user_ssh_keys: vec!["user_ssh_key".to_string()],
            grafana_admin_user: "grafana_admin_user".to_string(),
            grafana_admin_password: "grafana_admin_password".to_string(),
            qovery_engine_location: EngineLocation::QoverySide,
            cluster_maintenance_start_time: "06:00".to_string(),
            cluster_maintenance_end_time: None,
            tls_email_report: "".to_string(),
            // VPC related fields
            cluster_ipv4_cidr_block: None,
            services_ipv4_cidr_block: None,
            user_provided_network: None,
            vpc_qovery_network_mode: None,
            metrics_parameters: None,
        };

        // execute & validate:

        // case 1: Automatic VPC mode with default values (nothing specified)
        let gke_options_to_test = basic_gke_options.clone();
        assert_eq!(
            GkeVpcMode::Automatic {
                custom_cluster_ipv4_cidr_block: None,
                custom_services_ipv4_cidr_block: None,
            },
            gke_options_to_test
                .to_gke_vpc_mode()
                .expect("Cannot convert GkeOptions to GkeVpcMode")
        );

        // case 2: Automatic VPC mode with non default valid values
        let mut gke_options_to_test = basic_gke_options.clone();
        gke_options_to_test.cluster_ipv4_cidr_block = Some("10.10.10.1/18".to_string());
        gke_options_to_test.services_ipv4_cidr_block = Some("10.10.10.18/18".to_string());
        assert_eq!(
            GkeVpcMode::Automatic {
                custom_cluster_ipv4_cidr_block: Some(IpNet::from_str("10.10.10.1/18").expect("Cannot parse IP Net")),
                custom_services_ipv4_cidr_block: Some(IpNet::from_str("10.10.10.18/18").expect("Cannot parse IP Net")),
            },
            gke_options_to_test
                .to_gke_vpc_mode()
                .expect("Cannot convert GkeOptions to GkeVpcMode")
        );

        // case 3: User VPC mode with default values (nothing specified)
        let mut gke_options_to_test = basic_gke_options.clone();
        gke_options_to_test.user_provided_network = Some(UserProvidedVPCNetwork {
            vpc_project_id: Some("project_id".to_string()),
            vpc_name: "vpc_name".to_string(),
            subnetwork_name: Some("subnetwork_name".to_string()),
            ip_range_pods_name: Some("ip_range_pods_name".to_string()),
            additional_ip_range_pods_names: Some(vec![
                "additional_ip_range_pods_name_1".to_string(),
                "additional_ip_range_pods_name_2".to_string(),
            ]),
            ip_range_services_name: Some("ip_range_services_name".to_string()),
        });
        assert_eq!(
            GkeVpcMode::UserNetworkConfig {
                vpc_project_id: Some("project_id".to_string()),
                vpc_name: "vpc_name".to_string(),
                subnetwork_name: Some("subnetwork_name".to_string()),
                ip_range_pods_name: Some("ip_range_pods_name".to_string()),
                additional_ip_range_pods_names: Some(vec![
                    "additional_ip_range_pods_name_1".to_string(),
                    "additional_ip_range_pods_name_2".to_string(),
                ]),
                ip_range_services_name: Some("ip_range_services_name".to_string()),
            },
            gke_options_to_test
                .to_gke_vpc_mode()
                .expect("Cannot convert GkeOptions to GkeVpcMode")
        );
    }
}

use crate::cloud_provider::gcp::kubernetes::{GkeOptions as GkeOptionsModel, VpcMode as GkeVpcMode};
use crate::cloud_provider::models::VpcQoveryNetworkMode;
use crate::cloud_provider::qovery::EngineLocation;
use crate::models::gcp::io::JsonCredentials;
use crate::models::gcp::JsonCredentials as GkeJsonCredentials;
use ipnet::IpNet;
use serde::{de, Deserialize, Deserializer, Serialize};
use std::str::FromStr;
use time::macros::format_description;
use time::Time;

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

    // GCP
    #[serde(alias = "json_credentials")]
    #[serde(deserialize_with = "gcp_credentials_from_str")]
    // Allow to deserialize string field to its struct counterpart
    pub gcp_credentials: JsonCredentials,

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

/// Allow to properly deserialize JSON credentials from string, making sure to escape \n from keys strings
fn gcp_credentials_from_str<'de, D>(deserializer: D) -> Result<JsonCredentials, D::Error>
where
    D: Deserializer<'de>,
{
    let gcp_credentials: String = String::deserialize(deserializer)?;
    match JsonCredentials::try_new_from_json_str(&gcp_credentials) {
        Ok(credentials) => Ok(credentials),
        Err(e) => Err(de::Error::custom(e.to_string())),
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
            GkeJsonCredentials::try_from(value.gcp_credentials)
                .map_err(|e| format!("Cannot parse JSON credentials: {e}"))?,
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
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud_provider::gcp::kubernetes::VpcMode as GkeVpcMode;
    use crate::cloud_provider::qovery::EngineLocation;
    use ipnet::IpNet;
    use std::str::FromStr;

    const GKE_DEFAULT_GCP_JSON_CREDENTIALS_EXAMPLE: &str = r#"{
  "type": "service_account",
  "project_id": "gcp_project_id",
  "private_key_id": "gcp_json_credentials_private_key_id",
  "private_key": "gcp_json_credentials_private_key",
  "client_email": "gcp_json_credentials_client_email",
  "client_id": "gcp_json_credentials_client_id",
  "auth_uri": "gcp_json_credentials_auth_uri",
  "token_uri": "gcp_json_credentials_token_uri",
  "auth_provider_x509_cert_url": "gcp_json_credentials_auth_provider_x509_cert_url",
  "client_x509_cert_url": "gcp_json_credentials_client_x509_cert_url",
  "universe_domain": "gcp_json_credentials_universe_domain"
}"#;

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
            gcp_credentials: serde_json::from_str(GKE_DEFAULT_GCP_JSON_CREDENTIALS_EXAMPLE)
                .expect("Cannot deserialize JSON credentials from string"),
            cluster_maintenance_start_time: "06:00".to_string(),
            cluster_maintenance_end_time: None,
            tls_email_report: "".to_string(),
            // VPC related fields
            cluster_ipv4_cidr_block: None,
            services_ipv4_cidr_block: None,
            user_provided_network: None,
            vpc_qovery_network_mode: None,
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

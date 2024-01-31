use crate::cloud_provider::gcp::kubernetes::{GkeOptions as GkeOptionsModel, VpcMode as GkeVpcMode};
use crate::cloud_provider::qovery::EngineLocation;
use crate::models::gcp::io::JsonCredentials;
use crate::models::gcp::JsonCredentials as GkeJsonCredentials;
use ipnet::IpNet;
use serde::{de, Deserialize, Deserializer, Serialize};
use std::str::FromStr;
use time::macros::format_description;
use time::Time;

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(tag = "type")]
pub enum VpcMode {
    #[serde(rename = "AUTOMATIC")]
    Automatic {
        #[serde(skip_serializing_if = "Option::is_none")]
        custom_cluster_ipv4_cidr_block: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        custom_services_ipv4_cidr_block: Option<String>,
    },
    #[serde(rename = "EXISTING_VPC")]
    ExistingVpc {
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
    },
}

impl TryFrom<VpcMode> for GkeVpcMode {
    type Error = String;

    fn try_from(value: VpcMode) -> Result<Self, Self::Error> {
        match value {
            VpcMode::Automatic {
                custom_cluster_ipv4_cidr_block: cluster_ipv4_cidr_block,
                custom_services_ipv4_cidr_block: services_ipv4_cidr_block,
            } => Ok(GkeVpcMode::Automatic {
                custom_cluster_ipv4_cidr_block: match cluster_ipv4_cidr_block {
                    Some(cidr) => Some(
                        IpNet::from_str(cidr.as_str())
                            .map_err(|e| format!("cannot parse cluster_ipv4_cidr_block to IP Net: `{e}`"))?,
                    ),
                    None => None,
                },
                custom_services_ipv4_cidr_block: match services_ipv4_cidr_block {
                    Some(cidr) => Some(
                        IpNet::from_str(cidr.as_str())
                            .map_err(|e| format!("cannot parse services_ipv4_cidr_block to IP Net: `{e}`"))?,
                    ),
                    None => None,
                },
            }),
            VpcMode::ExistingVpc {
                vpc_project_id,
                vpc_name,
                subnetwork_name,
                ip_range_pods_name,
                additional_ip_range_pods_names: additional_ip_range_pods_name,
                ip_range_services_name,
            } => Ok(GkeVpcMode::ExistingVpc {
                vpc_project_id,
                vpc_name,
                subnetwork_name,
                ip_range_pods_name,
                additional_ip_range_pods_names: additional_ip_range_pods_name,
                ip_range_services_name,
            }),
        }
    }
}

#[derive(Serialize, Deserialize)]
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
    pub vpc_mode: VpcMode,

    // GCP to be checked during integration if needed:
    pub cluster_maintenance_start_time: String,
    #[serde(default)]
    pub cluster_maintenance_end_time: Option<String>,

    // Other
    pub tls_email_report: String,
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
        let vpc_mode: GkeVpcMode = value
            .vpc_mode
            .try_into()
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
    use crate::cloud_provider::gcp::kubernetes::io::VpcMode;

    #[test]
    fn test_vpc_mode_serialization() {
        // execute & validate:
        assert_eq!(
            r#"{"type":"AUTOMATIC"}"#.to_string().parse::<serde_json::Value>().expect("Cannot parse string to JSON"),
            serde_json::to_string(&VpcMode::Automatic {
                custom_cluster_ipv4_cidr_block: None,
                custom_services_ipv4_cidr_block: None
            })
            .expect("Cannot serialize VpcMode to string")
            .parse::<serde_json::Value>()
            .expect("Cannot parse string to JSON"),
        );
        assert_eq!(
            r#"{"type":"AUTOMATIC","custom_cluster_ipv4_cidr_block":"10.0.0.0/16","custom_services_ipv4_cidr_block":"10.4.0.0/16"}"#
                .parse::<serde_json::Value>()
                .expect("Cannot parse string to JSON"),
            serde_json::to_string(&VpcMode::Automatic {
                custom_cluster_ipv4_cidr_block: Some("10.0.0.0/16".to_string()),
                custom_services_ipv4_cidr_block: Some("10.4.0.0/16".to_string()),
            })
            .expect("Cannot serialize VpcMode to string")
            .parse::<serde_json::Value>()
            .expect("Cannot parse string to JSON"),
        );
        assert_eq!(
            r#"{"type":"EXISTING_VPC","vpc_project_id":"custom_vpc_project_id","vpc_name":"custom_vpc","subnetwork_name":"custom_vpc_subnetwork","ip_range_pods_name":"10.1.1.1/24","ip_range_services_name":"10.2.2.2/24","additional_ip_range_pods_names":["10.3.3.3/24","10.4.4.4/24"]}"#.to_string().parse::<serde_json::Value>().expect("Cannot parse string to JSON"),
            serde_json::to_string(&VpcMode::ExistingVpc {
                vpc_project_id: Some("custom_vpc_project_id".to_string()),
                vpc_name: "custom_vpc".to_string(),
                subnetwork_name: Some("custom_vpc_subnetwork".to_string()),
                ip_range_pods_name: Some("10.1.1.1/24".to_string()),
                ip_range_services_name: Some("10.2.2.2/24".to_string()),
                additional_ip_range_pods_names: Some(vec!["10.3.3.3/24".to_string(), "10.4.4.4/24".to_string()]),
            })
            .expect("Cannot serialize VpcMode to string")
            .parse::<serde_json::Value>().expect("Cannot parse string to JSON"),
        );
    }

    #[test]
    fn test_vpc_mode_deserialization() {
        // execute & validate:
        assert_eq!(
            VpcMode::Automatic {
                custom_cluster_ipv4_cidr_block: None,
                custom_services_ipv4_cidr_block: None,
            },
            serde_json::from_str(r#"{"type":"AUTOMATIC"}"#).expect("Cannot deserialize from string")
        );
        assert_eq!(
            VpcMode::Automatic {
                custom_cluster_ipv4_cidr_block: Some("10.0.0.0/16".to_string()),
                custom_services_ipv4_cidr_block: Some("10.4.0.0/16".to_string()),
            },
            serde_json::from_str(r#"{"type":"AUTOMATIC","custom_cluster_ipv4_cidr_block": "10.0.0.0/16","custom_services_ipv4_cidr_block": "10.4.0.0/16"}"#).expect("Cannot deserialize from string"),
        );
        assert_eq!(
            VpcMode::ExistingVpc {
                vpc_project_id: Some("custom_vpc_project_id".to_string()),
                vpc_name: "custom_vpc".to_string(),
                subnetwork_name: Some("custom_vpc_subnetwork".to_string()),
                ip_range_pods_name: Some("10.1.1.1/24".to_string()),
                ip_range_services_name: Some("10.2.2.2/24".to_string()),
                additional_ip_range_pods_names: Some(vec!["10.3.3.3/24".to_string(), "10.4.4.4/24".to_string()]),
            },
            serde_json::from_str(
                r#"{"type":"EXISTING_VPC","vpc_project_id":"custom_vpc_project_id","vpc_name":"custom_vpc","subnetwork_name":"custom_vpc_subnetwork","ip_range_pods_name":"10.1.1.1/24","ip_range_services_name":"10.2.2.2/24","additional_ip_range_pods_names":["10.3.3.3/24","10.4.4.4/24"]}"#
            )
            .expect("Cannot deserialize from string")
        );
    }
}

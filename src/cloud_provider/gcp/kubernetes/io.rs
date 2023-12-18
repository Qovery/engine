use crate::cloud_provider::gcp::kubernetes::{GkeOptions as GkeOptionsModel, VpcMode as GkeVpcMode};
use crate::cloud_provider::qovery::EngineLocation;
use crate::models::gcp::io::JsonCredentials;
use crate::models::gcp::JsonCredentials as GkeJsonCredentials;
use serde::{de, Deserialize, Deserializer, Serialize};
use time::macros::format_description;
use time::Time;

#[derive(Serialize, Deserialize, PartialEq, Debug)]
#[serde(tag = "type")]
pub enum VpcMode {
    #[serde(rename = "AUTOMATIC")]
    Automatic,
    #[serde(rename = "EXISTING_VPC")]
    ExistingVpc { vpc_name: String },
}

impl From<VpcMode> for GkeVpcMode {
    fn from(value: VpcMode) -> Self {
        match value {
            VpcMode::Automatic => GkeVpcMode::Automatic,
            VpcMode::ExistingVpc { vpc_name } => GkeVpcMode::ExistingVpc { vpc_name },
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
            value.vpc_mode.into(),
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
            r#"{"type":"AUTOMATIC"}"#.to_string(),
            serde_json::to_string(&VpcMode::Automatic).expect("Cannot serialize VpcMode to string")
        );
        assert_eq!(
            r#"{"type":"EXISTING_VPC","vpc_name":"custom_vpc"}"#.to_string(),
            serde_json::to_string(&VpcMode::ExistingVpc {
                vpc_name: "custom_vpc".to_string(),
            })
            .expect("Cannot serialize VpcMode to string")
        );
    }

    #[test]
    fn test_vpc_mode_deserialization() {
        // execute & validate:
        assert_eq!(
            VpcMode::Automatic,
            serde_json::from_str(r#"{"type":"AUTOMATIC"}"#).expect("Cannot deserialize from string")
        );
        assert_eq!(
            VpcMode::ExistingVpc {
                vpc_name: "custom_vpc".to_string(),
            },
            serde_json::from_str(r#"{"type":"EXISTING_VPC","vpc_name":"custom_vpc"}"#)
                .expect("Cannot deserialize from string")
        );
    }
}

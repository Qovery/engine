use crate::infrastructure::models::kubernetes::azure::AksOptions as AksOptionsModel;
use crate::io_models::engine_location::EngineLocation;
use crate::io_models::metrics::MetricsParameters;
use serde_derive::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct AksOptions {
    // Qovery
    pub qovery_api_url: String,
    pub qovery_grpc_url: String,
    pub qovery_engine_url: String,
    pub jwt_token: String,
    pub qovery_ssh_key: String,
    pub user_ssh_keys: Vec<String>,
    pub grafana_admin_user: String,
    pub grafana_admin_password: String,
    pub qovery_engine_location: EngineLocation,

    // Network
    // VPC

    // Other
    pub tls_email_report: String,
    #[serde(default)]
    pub metrics_parameters: Option<MetricsParameters>,

    // Azure specifics
    pub azure_resource_group_name: String,
}

impl AksOptions {}

impl TryFrom<AksOptions> for AksOptionsModel {
    type Error = String;

    fn try_from(value: AksOptions) -> Result<Self, Self::Error> {
        Ok(AksOptionsModel {
            qovery_api_url: value.qovery_api_url,
            qovery_grpc_url: value.qovery_grpc_url,
            qovery_engine_url: value.qovery_engine_url,
            jwt_token: value.jwt_token,
            qovery_ssh_key: value.qovery_ssh_key,
            user_ssh_keys: value.user_ssh_keys,
            grafana_admin_user: value.grafana_admin_user,
            grafana_admin_password: value.grafana_admin_password,
            qovery_engine_location: value.qovery_engine_location,
            metrics_parameters: value.metrics_parameters,
            tls_email_report: value.tls_email_report,
            azure_resource_group_name: value.azure_resource_group_name,
        })
    }
}

use crate::cloud_provider::kubernetes::ProviderOptions;
use crate::cloud_provider::qovery::EngineLocation;
use serde_derive::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    pub gcp_project_id: String,
    pub gcp_access_key: String,
    pub gcp_secret_key: String,

    // Other
    pub tls_email_report: String,
}

impl GkeOptions {
    pub fn _new(
        qovery_api_url: String,
        qovery_grpc_url: String,
        qovery_engine_url: String,
        jwt_token: String,
        qovery_ssh_key: String,
        user_ssh_keys: Vec<String>,
        grafana_admin_user: String,
        grafana_admin_password: String,
        qovery_engine_location: EngineLocation,
        gcp_project_id: String,
        gcp_access_key: String,
        gcp_secret_key: String,
        tls_email_report: String,
    ) -> Self {
        GkeOptions {
            qovery_api_url,
            qovery_grpc_url,
            qovery_engine_url,
            jwt_token,
            qovery_ssh_key,
            user_ssh_keys,
            grafana_admin_user,
            grafana_admin_password,
            qovery_engine_location,
            gcp_project_id,
            gcp_access_key,
            gcp_secret_key,
            tls_email_report,
        }
    }
}

impl ProviderOptions for GkeOptions {}

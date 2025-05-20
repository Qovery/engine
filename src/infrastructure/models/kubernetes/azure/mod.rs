use crate::io_models::engine_location::EngineLocation;
use crate::io_models::metrics::MetricsParameters;

pub mod aks;
mod node;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VpcMode {
    Automatic {},
    UserNetworkConfig {},
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
    // pub vpc_mode: VpcMode,
    // pub vpc_qovery_network_mode: Option<VpcQoveryNetworkMode>,

    // Other
    pub tls_email_report: String,
    pub metrics_parameters: Option<MetricsParameters>,
}

impl AksOptions {
    pub fn new(
        qovery_api_url: String,
        qovery_grpc_url: String,
        qovery_engine_url: String,
        jwt_token: String,
        qovery_ssh_key: String,
        user_ssh_keys: Vec<String>,
        grafana_admin_user: String,
        grafana_admin_password: String,
        qovery_engine_location: EngineLocation,
        tls_email_report: String,
        metrics_parameters: Option<MetricsParameters>,
    ) -> Self {
        AksOptions {
            qovery_api_url,
            qovery_grpc_url,
            qovery_engine_url,
            jwt_token,
            qovery_ssh_key,
            user_ssh_keys,
            grafana_admin_user,
            grafana_admin_password,
            qovery_engine_location,
            tls_email_report,
            metrics_parameters,
        }
    }
}

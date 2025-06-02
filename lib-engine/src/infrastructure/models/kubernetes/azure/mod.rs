use crate::environment::models::ToCloudProviderFormat;
use crate::io_models::engine_location::EngineLocation;
use crate::io_models::metrics::MetricsParameters;
use std::fmt::Display;

pub mod aks;
pub mod node;
pub mod node_group;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VpcMode {
    Automatic {},
    UserNetworkConfig {},
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkuTier {
    Free,
    Standard,
    Premium,
}

impl ToCloudProviderFormat for SkuTier {
    fn to_cloud_provider_format(&self) -> &str {
        match self {
            SkuTier::Free => "free",
            SkuTier::Standard => "standard",
            SkuTier::Premium => "premium",
        }
    }
}

impl Display for SkuTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                SkuTier::Free => "Free",
                SkuTier::Standard => "Standard",
                SkuTier::Premium => "Premium",
            }
        )
    }
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

    // Azure specifics
    pub azure_resource_group_name: String,
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
        azure_resource_group_name: String,
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
            azure_resource_group_name,
        }
    }
}

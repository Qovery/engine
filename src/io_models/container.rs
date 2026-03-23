use super::{PodAntiAffinity, TopologySpreadZone, UpdateStrategy};
use crate::environment::models;
use crate::environment::models::aws::AwsAppExtraSettings;
use crate::environment::models::azure::AzureAppExtraSettings;
use crate::environment::models::container::{ContainerError, ContainerService};
use crate::environment::models::gcp::GcpAppExtraSettings;
use crate::environment::models::registry_image_source::RegistryImageSource;
use crate::environment::models::scaleway::ScwAppExtraSettings;
use crate::environment::models::selfmanaged::OnPremiseAppExtraSettings;
use crate::environment::models::types::{AWS, Azure, GCP, OnPremise, SCW};
use crate::infrastructure::models::cloud_provider::aws::{AwsCredentials, new_rusoto_creds};
use crate::infrastructure::models::cloud_provider::io::{NginxConfigurationSnippet, NginxServerSnippet};
use crate::infrastructure::models::cloud_provider::{CloudProvider, Kind as CPKind};
use crate::infrastructure::models::container_registry::ecr::ECR;
use crate::infrastructure::models::container_registry::errors::ContainerRegistryError;
use crate::infrastructure::models::container_registry::{InteractWithRegistry, azure_container_registry};
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::io_models::annotations_group::AnnotationsGroup;
use crate::io_models::application::{PortIo, Storage, to_environment_variable};
use crate::io_models::container::keda_transform::{KEY, NAME, SECRET_TARGET_REF, strip_qovery_env_prefix};
use crate::io_models::context::Context;
use crate::io_models::labels_group::LabelsGroup;
use crate::io_models::models::{KubernetesCpuResourceUnit, KubernetesGpuResourceUnit, KubernetesMemoryResourceUnit};
use crate::io_models::probe::Probe;
use crate::io_models::variable_utils::{VariableInfo, default_environment_vars_with_info};
use crate::io_models::{Action, MountedFile};
use itertools::Itertools;
use rusoto_core::{Client, HttpClient, Region};
use rusoto_ecr::EcrClient;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;
use tracing::warn;
use url::Url;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash, Debug)]
pub struct Credentials {
    pub login: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
pub enum Registry {
    DockerHub {
        long_id: Uuid,
        url: Url,
        credentials: Option<Credentials>,
    },

    DoCr {
        long_id: Uuid,
        url: Url,
        token: String,
    },

    ScalewayCr {
        long_id: Uuid,
        url: Url,
        scaleway_access_key: String,
        scaleway_secret_key: String,
    },

    // AWS private ecr
    PrivateEcr {
        long_id: Uuid,
        url: Url,
        region: String,
        access_key_id: String,
        secret_access_key: String,
        #[serde(default)]
        session_token: Option<String>,
    },

    AzureCr {
        long_id: Uuid,
        url: Url,
        credentials: Option<Credentials>,
    },

    // AWS public ecr
    PublicEcr {
        long_id: Uuid,
        url: Url,
    },

    GenericCr {
        long_id: Uuid,
        url: Url,
        credentials: Option<Credentials>,
    },

    // GCP Artifact Registry
    GcpArtifactRegistry {
        long_id: Uuid,
        url: Url,
        credentials: Credentials,
    },
}

impl Registry {
    pub fn url(&self) -> &Url {
        match self {
            Registry::AzureCr { url, .. } => url,
            Registry::DockerHub { url, .. } => url,
            Registry::DoCr { url, .. } => url,
            Registry::ScalewayCr { url, .. } => url,
            Registry::PrivateEcr { url, .. } => url,
            Registry::PublicEcr { url, .. } => url,
            Registry::GenericCr { url, .. } => url,
            Registry::GcpArtifactRegistry { url, .. } => url,
        }
    }

    pub fn set_url(&mut self, mut new_url: Url) {
        let _ = new_url.set_username("");
        let _ = new_url.set_password(None);

        match self {
            Registry::AzureCr { url, .. } => *url = new_url,
            Registry::DockerHub { url, .. } => *url = new_url,
            Registry::DoCr { url, .. } => *url = new_url,
            Registry::ScalewayCr { url, .. } => *url = new_url,
            Registry::PrivateEcr { url, .. } => *url = new_url,
            Registry::PublicEcr { url, .. } => *url = new_url,
            Registry::GenericCr { url, .. } => *url = new_url,
            Registry::GcpArtifactRegistry { url, .. } => *url = new_url,
        }
    }

    pub fn id(&self) -> &Uuid {
        match self {
            Registry::AzureCr { long_id, .. } => long_id,
            Registry::DockerHub { long_id, .. } => long_id,
            Registry::DoCr { long_id, .. } => long_id,
            Registry::ScalewayCr { long_id, .. } => long_id,
            Registry::PrivateEcr { long_id, .. } => long_id,
            Registry::PublicEcr { long_id, .. } => long_id,
            Registry::GenericCr { long_id, .. } => long_id,
            Registry::GcpArtifactRegistry { long_id, .. } => long_id,
        }
    }

    pub fn name(&self) -> Option<String> {
        match self {
            Registry::AzureCr { url, .. } => Some(
                azure_container_registry::AzureContainerRegistry::get_registry_name_from_url(url).unwrap_or_default(),
            ),
            Registry::DockerHub { .. } => None,
            Registry::DoCr { .. } => None,
            Registry::ScalewayCr { .. } => None,
            Registry::PrivateEcr { .. } => None,
            Registry::PublicEcr { .. } => None,
            Registry::GenericCr { .. } => None,
            Registry::GcpArtifactRegistry { .. } => None,
        }
    }

    // Does some network calls for AWS/ECR
    pub fn get_url_with_credentials(&self) -> Result<Url, ContainerRegistryError> {
        let url = match self {
            Registry::AzureCr { url, credentials, .. } => {
                let mut url = url.clone();
                if let Some(credentials) = credentials {
                    let _ = url.set_username(&credentials.login);
                    let _ = url.set_password(Some(&credentials.password));
                }
                url
            }
            Registry::DockerHub { url, credentials, .. } => {
                let mut url = url.clone();
                if let Some(credentials) = credentials {
                    let _ = url.set_username(&credentials.login);
                    let _ = url.set_password(Some(&credentials.password));
                }
                url
            }
            Registry::DoCr { url, token, .. } => {
                let mut url = url.clone();
                let _ = url.set_username(token);
                let _ = url.set_password(Some(token));
                url
            }
            Registry::ScalewayCr {
                url,
                scaleway_access_key: _,
                scaleway_secret_key,
                ..
            } => {
                let mut url = url.clone();
                let _ = url.set_username("nologin");
                let _ = url.set_password(Some(scaleway_secret_key));
                url
            }
            Registry::PrivateEcr {
                long_id: _,
                url,
                region,
                access_key_id,
                secret_access_key,
                session_token,
            } => {
                let creds = new_rusoto_creds(&AwsCredentials::new(
                    access_key_id.to_string(),
                    secret_access_key.to_string(),
                    session_token.clone(),
                ));
                let region = Region::from_str(region).unwrap_or_default();
                let ecr_client =
                    EcrClient::new_with_client(Client::new_with(creds, HttpClient::new().unwrap()), region);
                let credentials = ECR::get_credentials(&ecr_client)?;
                let mut url = url.clone();
                let _ = url.set_username(&credentials.access_token);
                let _ = url.set_password(Some(&credentials.password));
                url
            }
            Registry::PublicEcr { url, .. } => url.clone(),
            Registry::GenericCr { url, credentials, .. } => {
                let mut url = url.clone();
                if let Some(credentials) = credentials {
                    let _ = url.set_username(&credentials.login);
                    let _ = url.set_password(Some(&credentials.password));
                }
                url
            }
            Registry::GcpArtifactRegistry { url, credentials, .. } => {
                let mut url = url.clone();
                let _ = url.set_username(&credentials.login);
                let _ = url.set_password(Some(&credentials.password));
                url
            }
        };

        Ok(url)
    }

    pub(crate) fn get_url(&self) -> Url {
        match self {
            Registry::AzureCr { url, .. } => url.clone(),
            Registry::DockerHub { url, .. } => url.clone(),
            Registry::DoCr { url, .. } => url.clone(),
            Registry::ScalewayCr { url, .. } => url.clone(),
            Registry::PrivateEcr { url, .. } => url.clone(),
            Registry::PublicEcr { url, .. } => url.clone(),
            Registry::GenericCr { url, .. } => url.clone(),
            Registry::GcpArtifactRegistry { url, .. } => url.clone(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
#[serde(default)]
pub struct ContainerAdvancedSettings {
    // Security
    #[serde(alias = "security.service_account_name")]
    pub security_service_account_name: String,
    #[serde(alias = "security.read_only_root_filesystem")]
    pub security_read_only_root_filesystem: bool,
    #[serde(alias = "security.automount_service_account_token")]
    pub security_automount_service_account_token: bool,

    // Deployment
    #[serde(alias = "deployment.termination_grace_period_seconds")]
    pub deployment_termination_grace_period_seconds: u32,
    #[serde(alias = "deployment.update_strategy.type")]
    pub deployment_update_strategy_type: UpdateStrategy,
    #[serde(alias = "deployment.update_strategy.rolling_update.max_unavailable_percent")]
    pub deployment_update_strategy_rolling_update_max_unavailable_percent: u32,
    #[serde(alias = "deployment.update_strategy.rolling_update.max_surge_percent")]
    pub deployment_update_strategy_rolling_update_max_surge_percent: u32,
    #[serde(alias = "deployment.affinity.node.required")]
    pub deployment_affinity_node_required: BTreeMap<String, String>,
    #[serde(alias = "deployment.antiaffinity.pod")]
    pub deployment_antiaffinity_pod: PodAntiAffinity,
    #[serde(alias = "deployment.topology_spread.zone")]
    pub deployment_topology_spread_zone: TopologySpreadZone,
    #[serde(alias = "deployment.lifecycle.post_start_exec_command")]
    pub deployment_lifecycle_post_start_exec_command: Vec<String>,
    #[serde(alias = "deployment.lifecycle.pre_stop_exec_command")]
    pub deployment_lifecycle_pre_stop_exec_command: Vec<String>,

    // Network DNS
    #[serde(alias = "network.dns.ndots")]
    pub network_dns_ndots: Option<u8>,

    // Gateway API
    #[serde(alias = "network.gateway_api.enable_sticky_session")]
    pub network_gateway_api_sticky_session_enable: bool,
    #[serde(alias = "network.gateway_api.force_ssl_redirect")]
    pub network_gateway_api_force_ssl_redirect: bool,
    #[serde(alias = "network.gateway_api.enable_cors")]
    pub network_gateway_api_enable_cors: bool,
    #[serde(alias = "network.gateway_api.cors_allow_origin")]
    pub network_gateway_api_cors_allow_origin: String,
    #[serde(alias = "network.gateway_api.cors_allow_methods")]
    pub network_gateway_api_cors_allow_methods: String,
    #[serde(alias = "network.gateway_api.cors_allow_headers")]
    pub network_gateway_api_cors_allow_headers: String,
    #[serde(alias = "network.gateway_api.whitelist_source_range")]
    pub network_gateway_api_whitelist_source_range: String,
    #[serde(alias = "network.gateway_api.denylist_source_range")]
    pub network_gateway_api_denylist_source_range: String,
    #[serde(alias = "network.gateway_api.basic_auth_env_var")]
    pub network_gateway_api_basic_auth_env_var: String,
    #[serde(alias = "network.gateway_api.route_limit_rpm")]
    pub network_gateway_api_route_limit_rpm: Option<u32>,
    #[serde(alias = "network.gateway_api.route_limit_rps")]
    pub network_gateway_api_route_limit_rps: Option<u32>,
    #[serde(alias = "network.gateway_api.route_limit_source_cidrs")]
    pub network_gateway_api_route_limit_source_cidrs: String,
    #[serde(alias = "network.gateway_api.route_limit_headers")]
    pub network_gateway_api_route_limit_headers: String,
    #[serde(alias = "network.gateway_api.add_headers")]
    pub network_gateway_api_add_headers: BTreeMap<String, String>,
    #[serde(alias = "network.gateway_api.proxy_set_headers")]
    pub network_gateway_api_proxy_set_headers: BTreeMap<String, String>,
    #[serde(
        alias = "network.gateway_api.custom_http_errors",
        with = "crate::io_models::types::http_status_codes"
    )]
    pub network_gateway_api_custom_http_errors: Option<Vec<u16>>,
    #[serde(alias = "network.gateway_api.circuit_breaker.max_connections")]
    pub network_gateway_api_circuit_breaker_max_connections: Option<u32>,
    #[serde(alias = "network.gateway_api.circuit_breaker.max_pending_requests")]
    pub network_gateway_api_circuit_breaker_max_pending_requests: Option<u32>,
    #[serde(alias = "network.gateway_api.circuit_breaker.max_parallel_requests")]
    pub network_gateway_api_circuit_breaker_max_parallel_requests: Option<u32>,
    #[serde(alias = "network.gateway_api.tcp_keepalive_idle_time_seconds")]
    pub network_gateway_api_tcp_keepalive_idle_time_seconds: Option<u32>,
    #[serde(alias = "network.gateway_api.tcp_keepalive_interval_seconds")]
    pub network_gateway_api_tcp_keepalive_interval_seconds: Option<u32>,
    #[serde(alias = "network.gateway_api.http_request_timeout_seconds")]
    pub network_gateway_api_http_request_timeout_seconds: Option<u32>,
    #[serde(alias = "network.gateway_api.http_connection_idle_timeout_seconds")]
    pub network_gateway_api_http_connection_idle_timeout_seconds: Option<u32>,

    // Ingress
    #[serde(alias = "network.ingress.proxy_body_size_mb")]
    pub network_ingress_proxy_body_size_mb: u32,
    #[serde(alias = "network.ingress.force_ssl_redirect")]
    pub network_ingress_force_ssl_redirect: bool, // TODO(benjaminch QOV-1400): to be removed
    #[serde(alias = "network.ingress.enable_cors")]
    pub network_ingress_cors_enable: bool, // TODO(benjaminch QOV-1400): to be removed
    #[serde(alias = "network.ingress.enable_sticky_session")]
    pub network_ingress_sticky_session_enable: bool, // TODO(benjaminch QOV-1400): to be removed
    #[serde(alias = "network.ingress.cors_allow_origin")]
    pub network_ingress_cors_allow_origin: String, // TODO(benjaminch QOV-1400): to be removed
    #[serde(alias = "network.ingress.cors_allow_methods")]
    pub network_ingress_cors_allow_methods: String, // TODO(benjaminch QOV-1400): to be removed
    #[serde(alias = "network.ingress.cors_allow_headers")]
    pub network_ingress_cors_allow_headers: String, // TODO(benjaminch QOV-1400): to be removed
    #[serde(alias = "network.ingress.keepalive_time_seconds")]
    pub network_ingress_keepalive_time_seconds: u32,
    #[serde(alias = "network.ingress.keepalive_timeout_seconds")]
    pub network_ingress_keepalive_timeout_seconds: u32,
    #[serde(alias = "network.ingress.send_timeout_seconds")]
    pub network_ingress_send_timeout_seconds: u32,
    #[serde(alias = "network.ingress.add_headers")]
    pub network_ingress_add_headers: BTreeMap<String, String>, // TODO(benjaminch QOV-1400): to be removed
    #[serde(alias = "network.ingress.proxy_set_headers")]
    pub network_ingress_proxy_set_headers: BTreeMap<String, String>, // TODO(benjaminch QOV-1400): to be removed
    #[serde(alias = "network.ingress.proxy_connect_timeout_seconds")]
    pub network_ingress_proxy_connect_timeout_seconds: u32,
    #[serde(alias = "network.ingress.proxy_send_timeout_seconds")]
    pub network_ingress_proxy_send_timeout_seconds: u32,
    #[serde(alias = "network.ingress.proxy_read_timeout_seconds")]
    pub network_ingress_proxy_read_timeout_seconds: u32, // TODO(benjaminch QOV-1400): to be removed
    #[serde(alias = "network.ingress.proxy_request_buffering")]
    pub network_ingress_proxy_request_buffering: String, // TODO(benjaminch QOV-1400): to be removed
    #[serde(alias = "network.ingress.proxy_buffering")]
    pub network_ingress_proxy_buffering: String, // TODO(benjaminch QOV-1400): to be removed
    #[serde(alias = "network.ingress.proxy_buffer_size_kb")]
    pub network_ingress_proxy_buffer_size_kb: u32, // TODO(benjaminch QOV-1400): to be removed
    #[serde(alias = "network.ingress.whitelist_source_range")]
    pub network_ingress_whitelist_source_range: String, // TODO(benjaminch QOV-1400): to be removed
    #[serde(alias = "network.ingress.denylist_source_range")]
    pub network_ingress_denylist_source_range: String, // TODO(benjaminch QOV-1400): to be removed
    #[serde(alias = "network.ingress.basic_auth_env_var")]
    pub network_ingress_basic_auth_env_var: String, // TODO(benjaminch QOV-1400): to be removed
    #[serde(alias = "network.ingress.nginx_controller_server_snippet")]
    pub network_ingress_nginx_controller_server_snippet: Option<NginxServerSnippet>,
    #[serde(alias = "network.ingress.nginx_controller_configuration_snippet")]
    pub network_ingress_nginx_controller_configuration_snippet: Option<NginxConfigurationSnippet>,
    #[serde(alias = "network.ingress.nginx_limit_rpm")]
    pub network_ingress_nginx_limit_rpm: Option<u32>, // TODO(benjaminch QOV-1400): to be removed
    #[serde(alias = "network.ingress.nginx_limit_rps")]
    pub network_ingress_nginx_limit_rps: Option<u32>, // TODO(benjaminch QOV-1400): to be removed
    #[serde(alias = "network.ingress.nginx_limit_burst_multiplier")]
    pub network_ingress_nginx_limit_burst_multiplier: Option<u32>, // TODO(benjaminch QOV-1400): to be removed
    #[serde(alias = "network.ingress.nginx_limit_connections")]
    pub network_ingress_nginx_limit_connections: Option<u32>, // TODO(benjaminch QOV-1400): to be removed
    #[serde(alias = "network.ingress.nginx_custom_http_errors")]
    pub network_ingress_nginx_custom_http_errors: Option<String>, // TODO(benjaminch QOV-1400): to be removed

    #[serde(alias = "network.ingress.grpc_send_timeout_seconds")]
    pub network_ingress_grpc_send_timeout_seconds: u32,
    #[serde(alias = "network.ingress.grpc_read_timeout_seconds")]
    pub network_ingress_grpc_read_timeout_seconds: u32,

    // Pod autoscaler
    #[serde(alias = "hpa.cpu.average_utilization_percent")]
    pub hpa_cpu_average_utilization_percent: u8,
    #[serde(alias = "hpa.memory.average_utilization_percent")]
    pub hpa_memory_average_utilization_percent: Option<u8>,
}

impl Default for ContainerAdvancedSettings {
    fn default() -> Self {
        ContainerAdvancedSettings {
            security_service_account_name: "".to_string(),
            security_read_only_root_filesystem: false,
            security_automount_service_account_token: false,
            deployment_termination_grace_period_seconds: 60,
            deployment_update_strategy_type: UpdateStrategy::RollingUpdate,
            deployment_update_strategy_rolling_update_max_unavailable_percent: 25,
            deployment_update_strategy_rolling_update_max_surge_percent: 25,
            deployment_affinity_node_required: BTreeMap::new(),
            deployment_antiaffinity_pod: PodAntiAffinity::Preferred,
            deployment_topology_spread_zone: TopologySpreadZone::Disabled,
            deployment_lifecycle_post_start_exec_command: vec![],
            deployment_lifecycle_pre_stop_exec_command: vec![],
            network_dns_ndots: None,
            network_ingress_proxy_body_size_mb: 100,
            network_ingress_force_ssl_redirect: true,
            network_ingress_cors_enable: false,
            network_ingress_sticky_session_enable: false,
            network_ingress_cors_allow_origin: "*".to_string(),
            network_ingress_cors_allow_methods: "GET, PUT, POST, DELETE, PATCH, OPTIONS".to_string(),
            network_ingress_cors_allow_headers: "DNT,Keep-Alive,User-Agent,X-Requested-With,If-Modified-Since,Cache-Control,Content-Type,Range,Authorization".to_string(),
            network_ingress_keepalive_time_seconds: 3600,
            network_ingress_keepalive_timeout_seconds: 60,
            network_ingress_send_timeout_seconds: 60,
            network_ingress_add_headers: BTreeMap::new(),
            network_ingress_proxy_set_headers: BTreeMap::new(),
            network_ingress_proxy_connect_timeout_seconds: 60,
            network_ingress_proxy_send_timeout_seconds: 60,
            network_ingress_proxy_read_timeout_seconds: 60,
            network_ingress_proxy_request_buffering: "on".to_string(),
            network_ingress_proxy_buffering: "on".to_string(),
            network_ingress_proxy_buffer_size_kb: 4,
            network_ingress_whitelist_source_range: "0.0.0.0/0".to_string(),
            network_ingress_denylist_source_range: "".to_string(),
            network_ingress_basic_auth_env_var: "".to_string(),
            network_ingress_grpc_send_timeout_seconds: 60,
            network_ingress_grpc_read_timeout_seconds: 60,
            network_ingress_nginx_limit_rpm: None,
            network_ingress_nginx_limit_rps: None,
            network_ingress_nginx_limit_burst_multiplier: None,
            network_ingress_nginx_limit_connections: None,
            network_ingress_nginx_controller_server_snippet: None,
            network_ingress_nginx_controller_configuration_snippet: None,
            network_ingress_nginx_custom_http_errors: None,
            network_gateway_api_sticky_session_enable: false,
            network_gateway_api_force_ssl_redirect: false,
            network_gateway_api_enable_cors: false,
            network_gateway_api_cors_allow_origin: "*".to_string(),
            network_gateway_api_cors_allow_methods: "GET, PUT, POST, DELETE, PATCH, OPTIONS".to_string(),
            network_gateway_api_cors_allow_headers: "DNT,Keep-Alive,User-Agent,X-Requested-With,If-Modified-Since,Cache-Control,Content-Type,Range,Authorization".to_string(),
            network_gateway_api_whitelist_source_range: "0.0.0.0/0".to_string(),
            network_gateway_api_denylist_source_range: "".to_string(),
            network_gateway_api_basic_auth_env_var: "".to_string(),
            network_gateway_api_route_limit_rpm: None,
            network_gateway_api_route_limit_rps: None,
            network_gateway_api_route_limit_source_cidrs: "".to_string(),
            network_gateway_api_route_limit_headers: "".to_string(),
            network_gateway_api_add_headers: BTreeMap::new(),
            network_gateway_api_proxy_set_headers: BTreeMap::new(),
            network_gateway_api_custom_http_errors: None,
            network_gateway_api_circuit_breaker_max_connections: None,
            network_gateway_api_circuit_breaker_max_pending_requests: None,
            network_gateway_api_circuit_breaker_max_parallel_requests: None,
            network_gateway_api_tcp_keepalive_idle_time_seconds: None,
            network_gateway_api_tcp_keepalive_interval_seconds: None,
            network_gateway_api_http_request_timeout_seconds: None,
            network_gateway_api_http_connection_idle_timeout_seconds: None,
            hpa_cpu_average_utilization_percent: 60,
            hpa_memory_average_utilization_percent: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash, Debug)]
pub struct KedaAuthenticationRef {
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash, Debug)]
pub struct KedaTriggerAuthentication {
    pub name: String,
    #[serde(default)]
    pub spec: Option<BTreeMap<String, serde_json::Value>>,
    #[serde(default)]
    pub raw_yaml: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash, Debug)]
pub struct KedaScaler {
    pub scaler_type: String,
    #[serde(default)]
    pub metadata: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub raw_yaml: Option<String>,
    #[serde(default)]
    pub authentication_ref: Option<KedaAuthenticationRef>,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash, Debug)]
pub struct KedaFallback {
    pub failure_threshold: i32,
    pub replicas: i32,
    #[serde(default)]
    pub behavior: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash, Debug)]
pub struct KedaConfig {
    #[serde(default)]
    pub polling_interval_seconds: Option<u32>,
    #[serde(default)]
    pub cooldown_period_seconds: Option<u32>,
    #[serde(default)]
    pub scalers: Vec<KedaScaler>,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AutoscalingConfig {
    Keda {
        #[serde(default)]
        polling_interval_seconds: Option<u32>,
        #[serde(default)]
        cooldown_period_seconds: Option<u32>,
        #[serde(default)]
        scalers: Vec<KedaScaler>,
        #[serde(default)]
        trigger_authentications: Vec<KedaTriggerAuthentication>,
        #[serde(default)]
        fallback: Option<KedaFallback>,
    },
}

impl KedaAuthenticationRef {
    fn to_domain(&self) -> models::autoscaling::KedaAuthenticationRef {
        models::autoscaling::KedaAuthenticationRef {
            name: self.name.clone(),
        }
    }
}

impl KedaScaler {
    fn to_domain(&self) -> models::autoscaling::KedaScaler {
        models::autoscaling::KedaScaler {
            scaler_type: self.scaler_type.clone(),
            metadata: self.metadata.clone(),
            raw_yaml: self.raw_yaml.clone(),
            authentication_ref: self.authentication_ref.as_ref().map(|a| a.to_domain()),
        }
    }
}

impl KedaFallback {
    fn to_domain(&self) -> models::autoscaling::KedaFallback {
        models::autoscaling::KedaFallback {
            failure_threshold: self.failure_threshold,
            replicas: self.replicas,
            behavior: self.behavior.clone(),
        }
    }
}

mod keda_transform {
    pub const QOVERY_ENV_PREFIX: &str = "qovery.env.";
    pub const SECRET_TARGET_REF: &str = "secretTargetRef";
    pub const KEY: &str = "key";
    pub const NAME: &str = "name";

    pub fn strip_qovery_env_prefix(value: &str) -> Option<&str> {
        value.strip_prefix(QOVERY_ENV_PREFIX)
    }
}

impl AutoscalingConfig {
    /// `secret_name` must match the Secret created by the Helm chart (`{{ service.name }}` / kube_name).
    pub fn to_domain(&self, secret_name: &str) -> models::autoscaling::AutoscalingConfig {
        match self {
            AutoscalingConfig::Keda {
                polling_interval_seconds,
                cooldown_period_seconds,
                scalers,
                trigger_authentications,
                fallback,
            } => {
                let transformed_trigger_auths = trigger_authentications
                    .iter()
                    .map(|trigger_auth| Self::process_trigger_authentication_secret_refs(trigger_auth, secret_name))
                    .collect();

                models::autoscaling::AutoscalingConfig::Keda {
                    polling_interval_seconds: *polling_interval_seconds,
                    cooldown_period_seconds: *cooldown_period_seconds,
                    scalers: scalers.iter().map(|s| s.to_domain()).collect(),
                    trigger_authentications: transformed_trigger_auths,
                    fallback: fallback.as_ref().map(|f| f.to_domain()),
                }
            }
        }
    }

    fn process_trigger_authentication_secret_refs(
        trigger_auth: &KedaTriggerAuthentication,
        secret_name: &str,
    ) -> models::autoscaling::KedaTriggerAuthentication {
        let transformed_spec = trigger_auth
            .spec
            .as_ref()
            .map(|spec| Self::transform_spec_secret_refs(spec, secret_name));

        let transformed_yaml = match &trigger_auth.raw_yaml {
            Some(yaml) => match Self::transform_yaml_secret_refs(yaml, secret_name, &trigger_auth.name) {
                Ok(result) => result,
                Err(err) => {
                    warn!(
                        trigger_auth_name = %trigger_auth.name,
                        error = %err,
                        "Failed to transform KEDA trigger authentication YAML, using original"
                    );
                    None
                }
            },
            None => None,
        };

        models::autoscaling::KedaTriggerAuthentication {
            name: trigger_auth.name.clone(),
            spec: transformed_spec,
            raw_yaml: transformed_yaml.or_else(|| trigger_auth.raw_yaml.clone()),
        }
    }

    fn transform_spec_secret_refs(
        spec: &BTreeMap<String, serde_json::Value>,
        secret_name: &str,
    ) -> BTreeMap<String, serde_json::Value> {
        let mut transformed_spec = spec.clone();

        if let Some(secret_target_ref) = transformed_spec.get_mut(SECRET_TARGET_REF)
            && let Some(arr) = secret_target_ref.as_array_mut()
        {
            for entry in arr {
                if let Some(obj) = entry.as_object_mut() {
                    let stripped_key = obj
                        .get(KEY)
                        .and_then(|v| v.as_str())
                        .and_then(strip_qovery_env_prefix)
                        .map(|s| s.to_string());

                    if let Some(key_without_prefix) = stripped_key {
                        obj.entry(NAME.to_string())
                            .or_insert_with(|| serde_json::Value::String(secret_name.to_string()));
                        obj.insert(KEY.to_string(), serde_json::Value::String(key_without_prefix));
                    }
                }
            }
        }

        transformed_spec
    }

    fn transform_yaml_secret_refs(
        raw_yaml: &str,
        secret_name: &str,
        trigger_auth_name: &str,
    ) -> Result<Option<String>, crate::errors::CommandError> {
        let mut yaml_value = serde_yaml::from_str::<serde_yaml::Value>(raw_yaml).map_err(|e| {
            crate::errors::CommandError::new(
                format!("Failed to parse YAML in KEDA trigger authentication '{trigger_auth_name}'"),
                Some(format!("YAML parse error: {e:?}")),
                None,
            )
        })?;

        let Some(secret_target_ref) = yaml_value.get_mut(SECRET_TARGET_REF) else {
            return Ok(None);
        };

        let Some(arr) = secret_target_ref.as_sequence_mut() else {
            return Ok(None);
        };

        let mut yaml_was_transformed = false;
        let key_yaml_value = serde_yaml::Value::String(KEY.to_string());
        let name_yaml_value = serde_yaml::Value::String(NAME.to_string());

        for entry in arr {
            let Some(mapping) = entry.as_mapping_mut() else {
                continue;
            };

            let stripped_key = mapping
                .get(&key_yaml_value)
                .and_then(|v| v.as_str())
                .and_then(strip_qovery_env_prefix)
                .map(|s| s.to_string());

            if let Some(key_without_prefix) = stripped_key {
                yaml_was_transformed = true;
                if !mapping.contains_key(&name_yaml_value) {
                    mapping.insert(name_yaml_value.clone(), serde_yaml::Value::String(secret_name.to_string()));
                }
                mapping.insert(key_yaml_value.clone(), serde_yaml::Value::String(key_without_prefix));
            }
        }

        if yaml_was_transformed {
            let transformed_yaml = serde_yaml::to_string(&yaml_value).map_err(|e| {
                crate::errors::CommandError::new(
                    format!("Failed to serialize YAML in KEDA trigger authentication '{trigger_auth_name}'"),
                    Some(format!("YAML serialize error: {e:?}")),
                    None,
                )
            })?;
            Ok(Some(transformed_yaml))
        } else {
            Ok(None)
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Container {
    pub long_id: Uuid,
    pub name: String,
    pub kube_name: String,
    pub action: Action,
    pub registry: Registry,
    pub image: String,
    pub tag: String,
    pub command_args: Vec<String>,
    pub entrypoint: Option<String>,
    pub cpu_request_in_milli: u32,
    pub cpu_limit_in_milli: u32,
    pub ram_request_in_mib: u32,
    pub ram_limit_in_mib: u32,
    pub gpu_request: Option<u32>,
    pub gpu_limit: Option<u32>,
    pub min_instances: u32,
    pub max_instances: u32,
    pub public_domain: String,
    pub ports: Vec<PortIo>,
    pub storages: Vec<Storage>,
    /// Key is a String, Value is a base64 encoded String
    /// Use BTreeMap to get Hash trait which is not available on HashMap
    #[serde(default = "default_environment_vars_with_info")]
    pub environment_vars_with_infos: BTreeMap<String, VariableInfo>,
    #[serde(default)]
    pub mounted_files: Vec<MountedFile>,
    pub readiness_probe: Option<Probe>,
    pub liveness_probe: Option<Probe>,
    #[serde(default)]
    pub advanced_settings: ContainerAdvancedSettings,
    #[serde(default)]
    pub annotations_group_ids: BTreeSet<Uuid>,
    #[serde(default)]
    pub labels_group_ids: BTreeSet<Uuid>,
    #[serde(default)]
    pub autoscaling: Option<AutoscalingConfig>,
}

impl Container {
    pub fn to_container_domain(
        mut self,
        context: &Context,
        cloud_provider: &dyn CloudProvider,
        default_container_registry: &dyn InteractWithRegistry,
        cluster: &dyn Kubernetes,
        annotations_group: &BTreeMap<Uuid, AnnotationsGroup>,
        labels_group: &BTreeMap<Uuid, LabelsGroup>,
    ) -> Result<Box<dyn ContainerService>, ContainerError> {
        let environment_variables = to_environment_variable(self.environment_vars_with_infos);

        // Default registry is a bit special as the core does not know its url/credentials as it is retrieved
        // by us with some tags
        if self.registry.id() == default_container_registry.long_id() {
            self.registry
                .set_url(default_container_registry.get_registry_endpoint(Some(cluster.cluster_name().as_str())));
        }

        let image_source = RegistryImageSource {
            registry: self.registry,
            image: self.image,
            tag: self.tag,
            registry_mirroring_mode: cluster.advanced_settings().registry_mirroring_mode.clone(),
        };
        let annotations_groups = self
            .annotations_group_ids
            .iter()
            .flat_map(|annotations_group_id| annotations_group.get(annotations_group_id))
            .cloned()
            .collect_vec();
        let labels_groups = self
            .labels_group_ids
            .iter()
            .flat_map(|labels_group_id| labels_group.get(labels_group_id))
            .cloned()
            .collect_vec();

        // Transform KEDA secretTargetRef qovery.env.* patterns during io_models → domain conversion
        let transformed_autoscaling = self.autoscaling.as_ref().map(|a| a.to_domain(&self.kube_name));

        let service: Box<dyn ContainerService> = match cloud_provider.kind() {
            CPKind::Aws => Box::new(models::container::Container::<AWS>::new(
                context,
                self.long_id,
                self.name,
                self.kube_name,
                self.action.to_service_action(),
                image_source,
                self.command_args,
                self.entrypoint,
                KubernetesCpuResourceUnit::MilliCpu(self.cpu_request_in_milli),
                KubernetesCpuResourceUnit::MilliCpu(self.cpu_limit_in_milli),
                KubernetesMemoryResourceUnit::MebiByte(self.ram_request_in_mib),
                KubernetesMemoryResourceUnit::MebiByte(self.ram_limit_in_mib),
                self.gpu_request.map(KubernetesGpuResourceUnit),
                self.gpu_limit.map(KubernetesGpuResourceUnit),
                self.min_instances,
                self.max_instances,
                self.public_domain,
                self.ports.iter().map(PortIo::to_port_domain).collect(),
                self.storages.iter().map(|s| s.to_storage()).collect::<Vec<_>>(),
                environment_variables,
                self.mounted_files
                    .iter()
                    .map(|e| e.to_domain())
                    .collect::<BTreeSet<_>>(),
                self.readiness_probe.map(|p| p.to_domain()),
                self.liveness_probe.map(|p| p.to_domain()),
                self.advanced_settings,
                AwsAppExtraSettings {},
                |transmitter| context.get_event_details(transmitter),
                annotations_groups,
                labels_groups,
                transformed_autoscaling.clone(),
            )?),
            CPKind::Azure => Box::new(models::container::Container::<Azure>::new(
                context,
                self.long_id,
                self.name,
                self.kube_name,
                self.action.to_service_action(),
                image_source,
                self.command_args,
                self.entrypoint,
                KubernetesCpuResourceUnit::MilliCpu(self.cpu_request_in_milli),
                KubernetesCpuResourceUnit::MilliCpu(self.cpu_limit_in_milli),
                KubernetesMemoryResourceUnit::MebiByte(self.ram_request_in_mib),
                KubernetesMemoryResourceUnit::MebiByte(self.ram_limit_in_mib),
                self.gpu_request.map(KubernetesGpuResourceUnit),
                self.gpu_limit.map(KubernetesGpuResourceUnit),
                self.min_instances,
                self.max_instances,
                self.public_domain,
                self.ports.iter().map(PortIo::to_port_domain).collect(),
                self.storages.iter().map(|s| s.to_storage()).collect::<Vec<_>>(),
                environment_variables,
                self.mounted_files
                    .iter()
                    .map(|e| e.to_domain())
                    .collect::<BTreeSet<_>>(),
                self.readiness_probe.map(|p| p.to_domain()),
                self.liveness_probe.map(|p| p.to_domain()),
                self.advanced_settings,
                AzureAppExtraSettings {},
                |transmitter| context.get_event_details(transmitter),
                annotations_groups,
                labels_groups,
                transformed_autoscaling.clone(),
            )?),
            CPKind::Scw => Box::new(models::container::Container::<SCW>::new(
                context,
                self.long_id,
                self.name,
                self.kube_name,
                self.action.to_service_action(),
                image_source,
                self.command_args,
                self.entrypoint,
                KubernetesCpuResourceUnit::MilliCpu(self.cpu_request_in_milli),
                KubernetesCpuResourceUnit::MilliCpu(self.cpu_limit_in_milli),
                KubernetesMemoryResourceUnit::MebiByte(self.ram_request_in_mib),
                KubernetesMemoryResourceUnit::MebiByte(self.ram_limit_in_mib),
                self.gpu_request.map(KubernetesGpuResourceUnit),
                self.gpu_limit.map(KubernetesGpuResourceUnit),
                self.min_instances,
                self.max_instances,
                self.public_domain,
                self.ports.iter().map(PortIo::to_port_domain).collect(),
                self.storages.iter().map(|s| s.to_storage()).collect::<Vec<_>>(),
                environment_variables,
                self.mounted_files
                    .iter()
                    .map(|e| e.to_domain())
                    .collect::<BTreeSet<_>>(),
                self.readiness_probe.map(|p| p.to_domain()),
                self.liveness_probe.map(|p| p.to_domain()),
                self.advanced_settings,
                ScwAppExtraSettings {},
                |transmitter| context.get_event_details(transmitter),
                annotations_groups,
                labels_groups,
                transformed_autoscaling.clone(),
            )?),
            CPKind::Gcp => Box::new(models::container::Container::<GCP>::new(
                context,
                self.long_id,
                self.name,
                self.kube_name,
                self.action.to_service_action(),
                image_source,
                self.command_args,
                self.entrypoint,
                KubernetesCpuResourceUnit::MilliCpu(self.cpu_request_in_milli),
                KubernetesCpuResourceUnit::MilliCpu(self.cpu_limit_in_milli),
                KubernetesMemoryResourceUnit::MebiByte(self.ram_request_in_mib),
                KubernetesMemoryResourceUnit::MebiByte(self.ram_limit_in_mib),
                self.gpu_request.map(KubernetesGpuResourceUnit),
                self.gpu_limit.map(KubernetesGpuResourceUnit),
                self.min_instances,
                self.max_instances,
                self.public_domain,
                self.ports.iter().map(PortIo::to_port_domain).collect(),
                self.storages.iter().map(|s| s.to_storage()).collect::<Vec<_>>(),
                environment_variables,
                self.mounted_files
                    .iter()
                    .map(|e| e.to_domain())
                    .collect::<BTreeSet<_>>(),
                self.readiness_probe.map(|p| p.to_domain()),
                self.liveness_probe.map(|p| p.to_domain()),
                self.advanced_settings,
                GcpAppExtraSettings {},
                |transmitter| context.get_event_details(transmitter),
                annotations_groups,
                labels_groups,
                transformed_autoscaling.clone(),
            )?),
            CPKind::OnPremise => Box::new(models::container::Container::<OnPremise>::new(
                context,
                self.long_id,
                self.name,
                self.kube_name,
                self.action.to_service_action(),
                image_source,
                self.command_args,
                self.entrypoint,
                KubernetesCpuResourceUnit::MilliCpu(self.cpu_request_in_milli),
                KubernetesCpuResourceUnit::MilliCpu(self.cpu_limit_in_milli),
                KubernetesMemoryResourceUnit::MebiByte(self.ram_request_in_mib),
                KubernetesMemoryResourceUnit::MebiByte(self.ram_limit_in_mib),
                self.gpu_request.map(KubernetesGpuResourceUnit),
                self.gpu_limit.map(KubernetesGpuResourceUnit),
                self.min_instances,
                self.max_instances,
                self.public_domain,
                self.ports.iter().map(PortIo::to_port_domain).collect(),
                self.storages.iter().map(|s| s.to_storage()).collect::<Vec<_>>(),
                environment_variables,
                self.mounted_files
                    .iter()
                    .map(|e| e.to_domain())
                    .collect::<BTreeSet<_>>(),
                self.readiness_probe.map(|p| p.to_domain()),
                self.liveness_probe.map(|p| p.to_domain()),
                self.advanced_settings,
                OnPremiseAppExtraSettings {},
                |transmitter| context.get_event_details(transmitter),
                annotations_groups,
                labels_groups,
                transformed_autoscaling.clone(),
            )?),
        };

        Ok(service)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{self, json};
    use std::collections::BTreeMap;

    #[test]
    fn test_autoscaling_config_deserialization() {
        let json = r#"{
            "type": "keda",
            "polling_interval_seconds": 30,
            "cooldown_period_seconds": 300,
            "scalers": [
                {
                    "scaler_type": "prometheus",
                    "raw_yaml": "serverAddress: http://prometheus-operated.prometheus.svc.cluster.local:9090\nquery: sum(rate(nginx_ingress_controller_requests{ingress=\"router-zc5ae9983-gaffotron\"}[2m]))\nthreshold: \"10\""
                }
            ]
        }"#;

        let result: Result<AutoscalingConfig, _> = serde_json::from_str(json);
        assert!(result.is_ok(), "Failed to deserialize: {:?}", result.err());

        let config = result.unwrap();
        match config {
            AutoscalingConfig::Keda {
                polling_interval_seconds,
                cooldown_period_seconds,
                scalers,
                trigger_authentications,
                ..
            } => {
                assert_eq!(polling_interval_seconds, Some(30));
                assert_eq!(cooldown_period_seconds, Some(300));
                assert_eq!(scalers.len(), 1);
                assert_eq!(scalers[0].scaler_type, "prometheus");
                assert_eq!(trigger_authentications.len(), 0);
            }
        }
    }

    #[test]
    fn test_autoscaling_config_serialization() {
        let config = AutoscalingConfig::Keda {
            polling_interval_seconds: Some(30),
            cooldown_period_seconds: Some(300),
            scalers: vec![KedaScaler {
                scaler_type: "prometheus".to_string(),
                metadata: None,
                raw_yaml: Some("serverAddress: http://test.com".to_string()),
                authentication_ref: None,
            }],
            trigger_authentications: vec![],
            fallback: None,
        };

        let json = serde_json::to_string_pretty(&config).unwrap();
        println!("Serialized JSON:\n{}", json);

        // Verify round-trip
        let deserialized: AutoscalingConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_autoscaling_config_none() {
        let json = r#"null"#;
        let result: Result<Option<AutoscalingConfig>, _> = serde_json::from_str(json);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_autoscaling_config_with_trigger_authentications() {
        let json = r#"{
            "type": "keda",
            "polling_interval_seconds": 30,
            "cooldown_period_seconds": 300,
            "scalers": [
                {
                    "scaler_type": "aws-sqs-queue",
                    "raw_yaml": "queueURL: https://sqs.eu-west-3.amazonaws.com/843237546537/qovery-z3f50657b\nawsRegion: eu-west-3\nqueueLength: \"5\"",
                    "authentication_ref": {
                        "name": "gaffotron-scaler-1-trigger-auth"
                    }
                }
            ],
            "trigger_authentications": [
                {
                    "name": "gaffotron-scaler-1-trigger-auth",
                    "raw_yaml": "podIdentity:\n  provider: aws\n  roleArn: arn:aws:iam::843237546537:role/keda-sqs-app1"
                }
            ]
        }"#;

        let result: Result<AutoscalingConfig, _> = serde_json::from_str(json);
        assert!(result.is_ok(), "Failed to deserialize: {:?}", result.err());

        let config = result.unwrap();
        match config {
            AutoscalingConfig::Keda {
                polling_interval_seconds,
                cooldown_period_seconds,
                scalers,
                trigger_authentications,
                ..
            } => {
                assert_eq!(polling_interval_seconds, Some(30));
                assert_eq!(cooldown_period_seconds, Some(300));

                // Check scalers
                assert_eq!(scalers.len(), 1);
                assert_eq!(scalers[0].scaler_type, "aws-sqs-queue");
                assert!(scalers[0].authentication_ref.is_some());
                assert_eq!(
                    scalers[0].authentication_ref.as_ref().unwrap().name,
                    "gaffotron-scaler-1-trigger-auth"
                );

                // Check trigger_authentications
                assert_eq!(trigger_authentications.len(), 1);
                assert_eq!(trigger_authentications[0].name, "gaffotron-scaler-1-trigger-auth");
                assert!(trigger_authentications[0].raw_yaml.is_some());
            }
        }
    }

    #[test]
    fn test_keda_trigger_authentication_with_secret_target_ref() {
        let json = r#"{
            "type": "keda",
            "scalers": [
                {
                    "scaler_type": "redis",
                    "authentication_ref": {
                        "name": "redis-trigger-auth"
                    }
                }
            ],
            "trigger_authentications": [
                {
                    "name": "redis-trigger-auth",
                    "spec": {
                        "secretTargetRef": [
                            {
                                "parameter": "password",
                                "key": "qovery.env.REDIS_SECRET"
                            },
                            {
                                "parameter": "host",
                                "key": "qovery.env.REDIS_HOST"
                            }
                        ]
                    }
                }
            ]
        }"#;

        let result: Result<AutoscalingConfig, _> = serde_json::from_str(json);
        assert!(result.is_ok(), "Failed to deserialize: {:?}", result.err());

        let config = result.unwrap();
        match config {
            AutoscalingConfig::Keda {
                trigger_authentications,
                ..
            } => {
                assert_eq!(trigger_authentications.len(), 1);
                assert_eq!(trigger_authentications[0].name, "redis-trigger-auth");
                assert!(trigger_authentications[0].spec.is_some());

                let spec = trigger_authentications[0].spec.as_ref().unwrap();
                let secret_target_ref = spec.get("secretTargetRef").unwrap();
                let arr = secret_target_ref.as_array().unwrap();

                assert_eq!(arr.len(), 2);

                // First entry
                let first = arr[0].as_object().unwrap();
                assert_eq!(first.get("parameter").unwrap().as_str().unwrap(), "password");
                assert_eq!(first.get("key").unwrap().as_str().unwrap(), "qovery.env.REDIS_SECRET");

                // Second entry
                let second = arr[1].as_object().unwrap();
                assert_eq!(second.get("parameter").unwrap().as_str().unwrap(), "host");
                assert_eq!(second.get("key").unwrap().as_str().unwrap(), "qovery.env.REDIS_HOST");
            }
        }
    }

    #[test]
    fn test_keda_trigger_authentication_mixed_secret_refs() {
        // Test with both qovery.env.* and regular keys
        let json = r#"{
            "type": "keda",
            "trigger_authentications": [
                {
                    "name": "mixed-auth",
                    "spec": {
                        "secretTargetRef": [
                            {
                                "parameter": "password",
                                "key": "qovery.env.DB_PASSWORD"
                            },
                            {
                                "parameter": "username",
                                "name": "existing-secret",
                                "key": "DB_USER"
                            }
                        ]
                    }
                }
            ]
        }"#;

        let result: Result<AutoscalingConfig, _> = serde_json::from_str(json);
        assert!(result.is_ok(), "Failed to deserialize: {:?}", result.err());

        let config = result.unwrap();
        match config {
            AutoscalingConfig::Keda {
                trigger_authentications,
                ..
            } => {
                assert_eq!(trigger_authentications.len(), 1);
                let spec = trigger_authentications[0].spec.as_ref().unwrap();
                let secret_target_ref = spec.get("secretTargetRef").unwrap();
                let arr = secret_target_ref.as_array().unwrap();

                assert_eq!(arr.len(), 2);

                // First entry should have qovery.env prefix
                let first = arr[0].as_object().unwrap();
                assert_eq!(first.get("key").unwrap().as_str().unwrap(), "qovery.env.DB_PASSWORD");
                assert!(first.get("name").is_none()); // No name yet (will be added by transformation)

                // Second entry should already have a name
                let second = arr[1].as_object().unwrap();
                assert_eq!(second.get("key").unwrap().as_str().unwrap(), "DB_USER");
                assert_eq!(second.get("name").unwrap().as_str().unwrap(), "existing-secret");
            }
        }
    }

    #[test]
    fn test_keda_trigger_authentication_raw_yaml_with_qovery_env() {
        // Test the exact format from user example
        let json = r#"{
            "type": "keda",
            "trigger_authentications": [
                {
                    "name": "16e83ba7-1768834139148-auth",
                    "raw_yaml": "secretTargetRef:\n- parameter: password\n  key: qovery.env.REDIS_SECRET"
                }
            ]
        }"#;

        let result: Result<AutoscalingConfig, _> = serde_json::from_str(json);
        assert!(result.is_ok(), "Failed to deserialize: {:?}", result.err());

        let config = result.unwrap();
        match config {
            AutoscalingConfig::Keda {
                trigger_authentications,
                ..
            } => {
                assert_eq!(trigger_authentications.len(), 1);
                assert_eq!(trigger_authentications[0].name, "16e83ba7-1768834139148-auth");
                assert!(trigger_authentications[0].raw_yaml.is_some());
                assert!(trigger_authentications[0].spec.is_none());

                let raw_yaml = trigger_authentications[0].raw_yaml.as_ref().unwrap();
                assert!(raw_yaml.contains("qovery.env.REDIS_SECRET"));
            }
        }
    }

    // Tests for KEDA secret refs transformation via to_domain()
    #[test]
    fn test_process_keda_secret_refs_with_qovery_env_prefix() {
        // Create a TriggerAuthentication with qovery.env.* pattern
        let mut spec = BTreeMap::new();
        spec.insert(
            "secretTargetRef".to_string(),
            json!([
                {
                    "parameter": "password",
                    "key": "qovery.env.REDIS_SECRET"
                },
                {
                    "parameter": "host",
                    "key": "qovery.env.REDIS_HOST"
                }
            ]),
        );

        let trigger_auth = KedaTriggerAuthentication {
            name: "redis-auth".to_string(),
            spec: Some(spec),
            raw_yaml: None,
        };

        let autoscaling = AutoscalingConfig::Keda {
            polling_interval_seconds: Some(30),
            cooldown_period_seconds: Some(300),
            scalers: vec![],
            trigger_authentications: vec![trigger_auth],
            fallback: None,
        };

        let secret_name = "app-test123-keda-redis-nginx";
        let result = autoscaling.to_domain(secret_name);

        match result {
            models::autoscaling::AutoscalingConfig::Keda {
                trigger_authentications,
                ..
            } => {
                assert_eq!(trigger_authentications.len(), 1);
                let spec = trigger_authentications[0].spec.as_ref().unwrap();
                let secret_target_ref = spec.get("secretTargetRef").unwrap();
                let arr = secret_target_ref.as_array().unwrap();

                // First entry
                let first = arr[0].as_object().unwrap();
                assert_eq!(first.get("parameter").unwrap().as_str().unwrap(), "password");
                assert_eq!(first.get("name").unwrap().as_str().unwrap(), secret_name);
                assert_eq!(first.get("key").unwrap().as_str().unwrap(), "REDIS_SECRET");

                // Second entry
                let second = arr[1].as_object().unwrap();
                assert_eq!(second.get("parameter").unwrap().as_str().unwrap(), "host");
                assert_eq!(second.get("name").unwrap().as_str().unwrap(), secret_name);
                assert_eq!(second.get("key").unwrap().as_str().unwrap(), "REDIS_HOST");
            }
        }
    }

    #[test]
    fn test_process_keda_secret_refs_without_qovery_env_prefix() {
        // Test that entries without qovery.env prefix are left unchanged
        let mut spec = BTreeMap::new();
        spec.insert(
            "secretTargetRef".to_string(),
            json!([
                {
                    "parameter": "username",
                    "name": "existing-secret",
                    "key": "DB_USER"
                }
            ]),
        );

        let trigger_auth = KedaTriggerAuthentication {
            name: "db-auth".to_string(),
            spec: Some(spec),
            raw_yaml: None,
        };

        let autoscaling = AutoscalingConfig::Keda {
            polling_interval_seconds: None,
            cooldown_period_seconds: None,
            scalers: vec![],
            trigger_authentications: vec![trigger_auth],
            fallback: None,
        };

        let secret_name = "my-service";
        let result = autoscaling.to_domain(secret_name);

        match result {
            models::autoscaling::AutoscalingConfig::Keda {
                trigger_authentications,
                ..
            } => {
                assert_eq!(trigger_authentications.len(), 1);
                let spec = trigger_authentications[0].spec.as_ref().unwrap();
                let secret_target_ref = spec.get("secretTargetRef").unwrap();
                let arr = secret_target_ref.as_array().unwrap();

                let entry = arr[0].as_object().unwrap();
                assert_eq!(entry.get("parameter").unwrap().as_str().unwrap(), "username");
                assert_eq!(entry.get("name").unwrap().as_str().unwrap(), "existing-secret");
                assert_eq!(entry.get("key").unwrap().as_str().unwrap(), "DB_USER");
            }
        }
    }

    #[test]
    fn test_process_keda_secret_refs_mixed() {
        // Test with both qovery.env.* and regular entries
        let mut spec = BTreeMap::new();
        spec.insert(
            "secretTargetRef".to_string(),
            json!([
                {
                    "parameter": "password",
                    "key": "qovery.env.DB_PASSWORD"
                },
                {
                    "parameter": "username",
                    "name": "external-secret",
                    "key": "USER"
                }
            ]),
        );

        let trigger_auth = KedaTriggerAuthentication {
            name: "mixed-auth".to_string(),
            spec: Some(spec),
            raw_yaml: None,
        };

        let autoscaling = AutoscalingConfig::Keda {
            polling_interval_seconds: Some(30),
            cooldown_period_seconds: Some(300),
            scalers: vec![],
            trigger_authentications: vec![trigger_auth],
            fallback: Some(KedaFallback {
                failure_threshold: 3,
                replicas: 2,
                behavior: None,
            }),
        };

        let secret_name = "my-app";
        let result = autoscaling.to_domain(secret_name);

        match result {
            models::autoscaling::AutoscalingConfig::Keda {
                trigger_authentications,
                fallback,
                ..
            } => {
                // Verify fallback is preserved
                assert!(fallback.is_some());
                assert_eq!(fallback.as_ref().unwrap().failure_threshold, 3);

                assert_eq!(trigger_authentications.len(), 1);
                let spec = trigger_authentications[0].spec.as_ref().unwrap();
                let secret_target_ref = spec.get("secretTargetRef").unwrap();
                let arr = secret_target_ref.as_array().unwrap();

                // First entry (qovery.env) should be transformed
                let first = arr[0].as_object().unwrap();
                assert_eq!(first.get("name").unwrap().as_str().unwrap(), secret_name);
                assert_eq!(first.get("key").unwrap().as_str().unwrap(), "DB_PASSWORD");

                // Second entry (regular) should remain unchanged
                let second = arr[1].as_object().unwrap();
                assert_eq!(second.get("name").unwrap().as_str().unwrap(), "external-secret");
                assert_eq!(second.get("key").unwrap().as_str().unwrap(), "USER");
            }
        }
    }

    #[test]
    fn test_process_keda_secret_refs_with_raw_yaml_no_secret_ref() {
        // Test that raw_yaml without secretTargetRef is left untouched
        let trigger_auth = KedaTriggerAuthentication {
            name: "raw-auth".to_string(),
            spec: None,
            raw_yaml: Some("podIdentity:\n  provider: aws\n  roleArn: arn:aws:iam::123:role/keda".to_string()),
        };

        let autoscaling = AutoscalingConfig::Keda {
            polling_interval_seconds: None,
            cooldown_period_seconds: None,
            scalers: vec![],
            trigger_authentications: vec![trigger_auth],
            fallback: None,
        };

        let secret_name = "my-service";
        let result = autoscaling.to_domain(secret_name);

        match result {
            models::autoscaling::AutoscalingConfig::Keda {
                trigger_authentications,
                ..
            } => {
                assert_eq!(trigger_authentications.len(), 1);
                assert!(trigger_authentications[0].raw_yaml.is_some());
                assert!(trigger_authentications[0].spec.is_none());
                // Verify content is unchanged
                assert!(
                    trigger_authentications[0]
                        .raw_yaml
                        .as_ref()
                        .unwrap()
                        .contains("podIdentity")
                );
            }
        }
    }

    #[test]
    fn test_process_keda_secret_refs_with_raw_yaml_with_qovery_env() {
        // Test raw_yaml with secretTargetRef containing qovery.env prefix
        let raw_yaml = "secretTargetRef:\n- parameter: password\n  key: qovery.env.REDIS_SECRET\n- parameter: host\n  key: qovery.env.REDIS_HOST";

        let trigger_auth = KedaTriggerAuthentication {
            name: "redis-auth".to_string(),
            spec: None,
            raw_yaml: Some(raw_yaml.to_string()),
        };

        let autoscaling = AutoscalingConfig::Keda {
            polling_interval_seconds: None,
            cooldown_period_seconds: None,
            scalers: vec![],
            trigger_authentications: vec![trigger_auth],
            fallback: None,
        };

        let secret_name = "app-test123-keda-redis-nginx";
        let result = autoscaling.to_domain(secret_name);

        match result {
            models::autoscaling::AutoscalingConfig::Keda {
                trigger_authentications,
                ..
            } => {
                assert_eq!(trigger_authentications.len(), 1);
                assert!(trigger_authentications[0].raw_yaml.is_some());

                let transformed_yaml = trigger_authentications[0].raw_yaml.as_ref().unwrap();

                // Verify the transformation occurred
                assert!(transformed_yaml.contains(&format!("name: {}", secret_name)));
                assert!(transformed_yaml.contains("key: REDIS_SECRET"));
                assert!(transformed_yaml.contains("key: REDIS_HOST"));
                assert!(!transformed_yaml.contains("qovery.env."));

                // Parse to verify structure
                let parsed: serde_yaml::Value = serde_yaml::from_str(transformed_yaml).unwrap();
                let secret_refs = parsed.get("secretTargetRef").unwrap().as_sequence().unwrap();

                assert_eq!(secret_refs.len(), 2);

                // First entry
                let first = secret_refs[0].as_mapping().unwrap();
                assert_eq!(
                    first
                        .get(serde_yaml::Value::String("parameter".to_string()))
                        .unwrap()
                        .as_str()
                        .unwrap(),
                    "password"
                );
                assert_eq!(
                    first
                        .get(serde_yaml::Value::String("name".to_string()))
                        .unwrap()
                        .as_str()
                        .unwrap(),
                    secret_name
                );
                assert_eq!(
                    first
                        .get(serde_yaml::Value::String("key".to_string()))
                        .unwrap()
                        .as_str()
                        .unwrap(),
                    "REDIS_SECRET"
                );
            }
        }
    }

    #[test]
    fn test_process_keda_secret_refs_raw_yaml_mixed() {
        // Test raw_yaml with both qovery.env and regular entries
        let raw_yaml = "secretTargetRef:\n- parameter: password\n  key: qovery.env.DB_PASSWORD\n- parameter: username\n  name: external-secret\n  key: DB_USER";

        let trigger_auth = KedaTriggerAuthentication {
            name: "mixed-auth".to_string(),
            spec: None,
            raw_yaml: Some(raw_yaml.to_string()),
        };

        let autoscaling = AutoscalingConfig::Keda {
            polling_interval_seconds: None,
            cooldown_period_seconds: None,
            scalers: vec![],
            trigger_authentications: vec![trigger_auth],
            fallback: None,
        };

        let secret_name = "my-app";
        let result = autoscaling.to_domain(secret_name);

        match result {
            models::autoscaling::AutoscalingConfig::Keda {
                trigger_authentications,
                ..
            } => {
                assert_eq!(trigger_authentications.len(), 1);
                let transformed_yaml = trigger_authentications[0].raw_yaml.as_ref().unwrap();

                // Parse to verify structure
                let parsed: serde_yaml::Value = serde_yaml::from_str(transformed_yaml).unwrap();
                let secret_refs = parsed.get("secretTargetRef").unwrap().as_sequence().unwrap();

                // First entry should be transformed
                let first = secret_refs[0].as_mapping().unwrap();
                assert_eq!(
                    first
                        .get(serde_yaml::Value::String("name".to_string()))
                        .unwrap()
                        .as_str()
                        .unwrap(),
                    secret_name
                );
                assert_eq!(
                    first
                        .get(serde_yaml::Value::String("key".to_string()))
                        .unwrap()
                        .as_str()
                        .unwrap(),
                    "DB_PASSWORD"
                );

                // Second entry should remain unchanged
                let second = secret_refs[1].as_mapping().unwrap();
                assert_eq!(
                    second
                        .get(serde_yaml::Value::String("name".to_string()))
                        .unwrap()
                        .as_str()
                        .unwrap(),
                    "external-secret"
                );
                assert_eq!(
                    second
                        .get(serde_yaml::Value::String("key".to_string()))
                        .unwrap()
                        .as_str()
                        .unwrap(),
                    "DB_USER"
                );
            }
        }
    }

    #[test]
    fn test_process_keda_secret_refs_preserves_existing_name() {
        let mut spec = BTreeMap::new();
        spec.insert(
            "secretTargetRef".to_string(),
            json!([
                {
                    "parameter": "password",
                    "name": "custom-secret",
                    "key": "qovery.env.DB_PASSWORD"
                }
            ]),
        );

        let trigger_auth = KedaTriggerAuthentication {
            name: "db-auth".to_string(),
            spec: Some(spec),
            raw_yaml: None,
        };

        let autoscaling = AutoscalingConfig::Keda {
            polling_interval_seconds: None,
            cooldown_period_seconds: None,
            scalers: vec![],
            trigger_authentications: vec![trigger_auth],
            fallback: None,
        };

        let secret_name = "my-service";
        let result = autoscaling.to_domain(secret_name);

        match result {
            models::autoscaling::AutoscalingConfig::Keda {
                trigger_authentications,
                ..
            } => {
                assert_eq!(trigger_authentications.len(), 1);
                let spec = trigger_authentications[0].spec.as_ref().unwrap();
                let secret_target_ref = spec.get("secretTargetRef").unwrap();
                let arr = secret_target_ref.as_array().unwrap();
                let entry = arr[0].as_object().unwrap();

                assert_eq!(entry.get("name").unwrap().as_str().unwrap(), "custom-secret");
                assert_eq!(entry.get("key").unwrap().as_str().unwrap(), "DB_PASSWORD");
            }
        }
    }

    #[test]
    fn test_process_keda_secret_refs_raw_yaml_parse_error_best_effort() {
        let invalid_yaml = "secretTargetRef: [";

        let trigger_auth = KedaTriggerAuthentication {
            name: "invalid-auth".to_string(),
            spec: None,
            raw_yaml: Some(invalid_yaml.to_string()),
        };

        let autoscaling = AutoscalingConfig::Keda {
            polling_interval_seconds: None,
            cooldown_period_seconds: None,
            scalers: vec![],
            trigger_authentications: vec![trigger_auth],
            fallback: None,
        };

        let result = autoscaling.to_domain("svc-name");

        match result {
            models::autoscaling::AutoscalingConfig::Keda {
                trigger_authentications,
                ..
            } => {
                assert_eq!(trigger_authentications.len(), 1);
                assert!(trigger_authentications[0].spec.is_none());
                assert_eq!(trigger_authentications[0].raw_yaml.as_deref(), Some(invalid_yaml));
            }
        }
    }
}

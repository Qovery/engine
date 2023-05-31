use crate::cloud_provider::kubernetes::Kind as KubernetesKind;
use crate::cloud_provider::{CloudProvider, Kind as CPKind};
use crate::container_registry::ecr::ECR;
use crate::container_registry::ContainerRegistry;
use crate::io_models::application::{to_environment_variable, Port, Storage};
use crate::io_models::context::Context;
use crate::io_models::probe::Probe;
use crate::io_models::{Action, MountedFile};
use crate::models;
use crate::models::aws::AwsAppExtraSettings;
use crate::models::aws_ec2::AwsEc2AppExtraSettings;
use crate::models::container::{ContainerError, ContainerService};
use crate::models::scaleway::ScwAppExtraSettings;
use crate::models::types::{AWSEc2, AWS, SCW};
use rusoto_core::{Client, HttpClient, Region};
use rusoto_credential::StaticProvider;
use rusoto_ecr::EcrClient;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;
use url::Url;
use uuid::Uuid;

use super::UpdateStrategy;

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash, Debug)]
pub struct Credentials {
    pub login: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
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
    },

    // AWS public ecr
    PublicEcr {
        long_id: Uuid,
        url: Url,
    },
}

impl Registry {
    pub fn url(&self) -> &Url {
        match self {
            Registry::DockerHub { url, .. } => url,
            Registry::DoCr { url, .. } => url,
            Registry::ScalewayCr { url, .. } => url,
            Registry::PrivateEcr { url, .. } => url,
            Registry::PublicEcr { url, .. } => url,
        }
    }

    pub fn set_url(&mut self, mut new_url: Url) {
        let _ = new_url.set_username("");
        let _ = new_url.set_password(None);

        match self {
            Registry::DockerHub { ref mut url, .. } => *url = new_url,
            Registry::DoCr { ref mut url, .. } => *url = new_url,
            Registry::ScalewayCr { ref mut url, .. } => *url = new_url,
            Registry::PrivateEcr { ref mut url, .. } => *url = new_url,
            Registry::PublicEcr { ref mut url, .. } => *url = new_url,
        }
    }

    pub fn id(&self) -> &Uuid {
        match self {
            Registry::DockerHub { long_id, .. } => long_id,
            Registry::DoCr { long_id, .. } => long_id,
            Registry::ScalewayCr { long_id, .. } => long_id,
            Registry::PrivateEcr { long_id, .. } => long_id,
            Registry::PublicEcr { long_id, .. } => long_id,
        }
    }

    // Does some network calls for AWS/ECR
    pub fn get_url_with_credentials(&self) -> Url {
        let url = match self {
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
                url: _,
                region,
                access_key_id,
                secret_access_key,
                ..
            } => {
                let creds = StaticProvider::new(access_key_id.to_string(), secret_access_key.to_string(), None, None);
                let region = Region::from_str(region).unwrap_or_default();
                let ecr_client =
                    EcrClient::new_with_client(Client::new_with(creds, HttpClient::new().unwrap()), region);

                let credentials = ECR::get_credentials(&ecr_client).unwrap();
                let mut url = Url::parse(credentials.endpoint_url.as_str()).unwrap();
                let _ = url.set_username(&credentials.access_token);
                let _ = url.set_password(Some(&credentials.password));
                url
            }
            Registry::PublicEcr { url, .. } => url.clone(),
        };

        url
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
#[serde(default)]
pub struct ContainerAdvancedSettings {
    // Security
    #[serde(alias = "security.service_account_name")]
    pub security_service_account_name: String,

    // Deployment
    #[serde(alias = "deployment.custom_domain_check_enabled")]
    pub deployment_custom_domain_check_enabled: bool,
    #[serde(alias = "deployment.termination_grace_period_seconds")]
    pub deployment_termination_grace_period_seconds: u32,
    #[serde(alias = "deployment.update_strategy.type")]
    pub deployment_update_strategy_type: UpdateStrategy,
    #[serde(alias = "deployment.update_strategy.rolling_update.max_unavailable_percent")]
    pub deployment_update_strategy_rolling_update_max_unavailable_percent: u32,
    #[serde(alias = "deployment.update_strategy.rolling_update.max_surge_percent")]
    pub deployment_update_strategy_rolling_update_max_surge_percent: u32,

    // Ingress
    #[serde(alias = "network.ingress.proxy_body_size_mb")]
    pub network_ingress_proxy_body_size_mb: u32,
    #[serde(alias = "network.ingress.enable_cors")]
    pub network_ingress_cors_enable: bool,
    #[serde(alias = "network.ingress.enable_sticky_session")]
    pub network_ingress_sticky_session_enable: bool,
    #[serde(alias = "network.ingress.cors_allow_origin")]
    pub network_ingress_cors_allow_origin: String,
    #[serde(alias = "network.ingress.cors_allow_methods")]
    pub network_ingress_cors_allow_methods: String,
    #[serde(alias = "network.ingress.cors_allow_headers")]
    pub network_ingress_cors_allow_headers: String,
    #[serde(alias = "network.ingress.keepalive_time_seconds")]
    pub network_ingress_keepalive_time_seconds: u32,
    #[serde(alias = "network.ingress.keepalive_timeout_seconds")]
    pub network_ingress_keepalive_timeout_seconds: u32,
    #[serde(alias = "network.ingress.send_timeout_seconds")]
    pub network_ingress_send_timeout_seconds: u32,
    #[serde(alias = "network.ingress.proxy_connect_timeout_seconds")]
    pub network_ingress_proxy_connect_timeout_seconds: u32,
    #[serde(alias = "network.ingress.proxy_send_timeout_seconds")]
    pub network_ingress_proxy_send_timeout_seconds: u32,
    #[serde(alias = "network.ingress.proxy_read_timeout_seconds")]
    pub network_ingress_proxy_read_timeout_seconds: u32,
    #[serde(alias = "network.ingress.proxy_buffer_size_kb")]
    pub network_ingress_proxy_buffer_size_kb: u32,
    #[serde(alias = "network.ingress.whitelist_source_range")]
    pub network_ingress_whitelist_source_range: String,
    #[serde(alias = "network.ingress.denylist_source_range")]
    pub network_ingress_denylist_source_range: String,
    #[serde(alias = "network.ingress.basic_auth_env_var")]
    pub network_ingress_basic_auth_env_var: String,

    #[serde(alias = "network.ingress.grpc_send_timeout_seconds")]
    pub network_ingress_grpc_send_timeout_seconds: u32,
    #[serde(alias = "network.ingress.grpc_read_timeout_seconds")]
    pub network_ingress_grpc_read_timeout_seconds: u32,

    // Pod autoscaler
    #[serde(alias = "hpa.cpu.average_utilization_percent")]
    pub hpa_cpu_average_utilization_percent: u8,
}

impl Default for ContainerAdvancedSettings {
    fn default() -> Self {
        ContainerAdvancedSettings {
            security_service_account_name: "".to_string(),
            deployment_termination_grace_period_seconds: 60,
            deployment_custom_domain_check_enabled: true,
            deployment_update_strategy_type: UpdateStrategy::RollingUpdate,
            deployment_update_strategy_rolling_update_max_unavailable_percent: 25,
            deployment_update_strategy_rolling_update_max_surge_percent: 25,
            network_ingress_proxy_body_size_mb: 100,
            network_ingress_cors_enable: false,
            network_ingress_sticky_session_enable: false,
            network_ingress_cors_allow_origin: "*".to_string(),
            network_ingress_cors_allow_methods: "GET, PUT, POST, DELETE, PATCH, OPTIONS".to_string(),
            network_ingress_cors_allow_headers: "DNT,Keep-Alive,User-Agent,X-Requested-With,If-Modified-Since,Cache-Control,Content-Type,Range,Authorization".to_string(),
            network_ingress_keepalive_time_seconds: 3600,
            network_ingress_keepalive_timeout_seconds: 60,
            network_ingress_send_timeout_seconds: 60,
            network_ingress_proxy_connect_timeout_seconds: 60,
            network_ingress_proxy_send_timeout_seconds: 60,
            network_ingress_proxy_read_timeout_seconds: 60,
            network_ingress_proxy_buffer_size_kb: 4,
            network_ingress_whitelist_source_range: "0.0.0.0/0".to_string(),
            network_ingress_denylist_source_range: "".to_string(),
            network_ingress_basic_auth_env_var: "".to_string(),
            network_ingress_grpc_send_timeout_seconds: 60,
            network_ingress_grpc_read_timeout_seconds: 60,
            hpa_cpu_average_utilization_percent: 60,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Container {
    pub long_id: Uuid,
    pub name: String,
    pub action: Action,
    pub registry: Registry,
    pub image: String,
    pub tag: String,
    pub command_args: Vec<String>,
    pub entrypoint: Option<String>,
    pub cpu_request_in_mili: u32,
    pub cpu_limit_in_mili: u32,
    pub ram_request_in_mib: u32,
    pub ram_limit_in_mib: u32,
    pub min_instances: u32,
    pub max_instances: u32,
    pub ports: Vec<Port>,
    pub storages: Vec<Storage>,
    /// Key is a String, Value is a base64 encoded String
    /// Use BTreeMap to get Hash trait which is not available on HashMap
    pub environment_vars: BTreeMap<String, String>,
    #[serde(default)]
    pub mounted_files: Vec<MountedFile>,
    pub readiness_probe: Option<Probe>,
    pub liveness_probe: Option<Probe>,
    #[serde(default)]
    pub advanced_settings: ContainerAdvancedSettings,
}

impl Container {
    pub fn to_container_domain(
        mut self,
        context: &Context,
        cloud_provider: &dyn CloudProvider,
        default_container_registry: &dyn ContainerRegistry,
    ) -> Result<Box<dyn ContainerService>, ContainerError> {
        let environment_variables = to_environment_variable(self.environment_vars);

        // Default registry is a bit special as the core does not knows its url/credentials as it is retrieved
        // by us with some tags
        if self.registry.id() == default_container_registry.long_id() {
            self.registry
                .set_url(default_container_registry.registry_info().endpoint.clone());
        }

        let service: Box<dyn ContainerService> = match cloud_provider.kind() {
            CPKind::Aws => {
                if cloud_provider.kubernetes_kind() == KubernetesKind::Eks {
                    Box::new(models::container::Container::<AWS>::new(
                        context,
                        self.long_id,
                        self.name,
                        self.action.to_service_action(),
                        self.registry,
                        self.image,
                        self.tag,
                        self.command_args,
                        self.entrypoint,
                        self.cpu_request_in_mili,
                        self.cpu_limit_in_mili,
                        self.ram_request_in_mib,
                        self.ram_limit_in_mib,
                        self.min_instances,
                        self.max_instances,
                        self.ports,
                        self.storages.iter().map(|s| s.to_aws_storage()).collect::<Vec<_>>(),
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
                    )?)
                } else {
                    Box::new(models::container::Container::<AWSEc2>::new(
                        context,
                        self.long_id,
                        self.name,
                        self.action.to_service_action(),
                        self.registry,
                        self.image,
                        self.tag,
                        self.command_args,
                        self.entrypoint,
                        self.cpu_request_in_mili,
                        self.cpu_limit_in_mili,
                        self.ram_request_in_mib,
                        self.ram_limit_in_mib,
                        self.min_instances,
                        self.max_instances,
                        self.ports,
                        self.storages.iter().map(|s| s.to_aws_ec2_storage()).collect::<Vec<_>>(),
                        environment_variables,
                        self.mounted_files
                            .iter()
                            .map(|e| e.to_domain())
                            .collect::<BTreeSet<_>>(),
                        self.readiness_probe.map(|p| p.to_domain()),
                        self.liveness_probe.map(|p| p.to_domain()),
                        self.advanced_settings,
                        AwsEc2AppExtraSettings {},
                        |transmitter| context.get_event_details(transmitter),
                    )?)
                }
            }
            CPKind::Scw => Box::new(models::container::Container::<SCW>::new(
                context,
                self.long_id,
                self.name,
                self.action.to_service_action(),
                self.registry,
                self.image,
                self.tag,
                self.command_args,
                self.entrypoint,
                self.cpu_request_in_mili,
                self.cpu_limit_in_mili,
                self.ram_request_in_mib,
                self.ram_limit_in_mib,
                self.min_instances,
                self.max_instances,
                self.ports,
                self.storages.iter().map(|s| s.to_scw_storage()).collect::<Vec<_>>(),
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
            )?),
        };

        Ok(service)
    }
}

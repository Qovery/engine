use super::{PodAntiAffinity, UpdateStrategy};
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
use crate::io_models::application::{Port, Storage, to_environment_variable};
use crate::io_models::context::Context;
use crate::io_models::labels_group::LabelsGroup;
use crate::io_models::models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};
use crate::io_models::probe::Probe;
use crate::io_models::variable_utils::{VariableInfo, default_environment_vars_with_info};
use crate::io_models::{Action, MountedFile};
use itertools::Itertools;
use rusoto_core::{Client, HttpClient, Region};
use rusoto_ecr::EcrClient;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;
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
    #[serde(alias = "deployment.lifecycle.post_start_exec_command")]
    pub deployment_lifecycle_post_start_exec_command: Vec<String>,
    #[serde(alias = "deployment.lifecycle.pre_stop_exec_command")]
    pub deployment_lifecycle_pre_stop_exec_command: Vec<String>,

    // Ingress
    #[serde(alias = "network.ingress.proxy_body_size_mb")]
    pub network_ingress_proxy_body_size_mb: u32,
    #[serde(alias = "network.ingress.force_ssl_redirect")]
    pub network_ingress_force_ssl_redirect: bool,
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
    #[serde(alias = "network.ingress.add_headers")]
    pub network_ingress_add_headers: BTreeMap<String, String>,
    #[serde(alias = "network.ingress.proxy_set_headers")]
    pub network_ingress_proxy_set_headers: BTreeMap<String, String>,
    #[serde(alias = "network.ingress.proxy_connect_timeout_seconds")]
    pub network_ingress_proxy_connect_timeout_seconds: u32,
    #[serde(alias = "network.ingress.proxy_send_timeout_seconds")]
    pub network_ingress_proxy_send_timeout_seconds: u32,
    #[serde(alias = "network.ingress.proxy_read_timeout_seconds")]
    pub network_ingress_proxy_read_timeout_seconds: u32,
    #[serde(alias = "network.ingress.proxy_request_buffering")]
    pub network_ingress_proxy_request_buffering: String,
    #[serde(alias = "network.ingress.proxy_buffering")]
    pub network_ingress_proxy_buffering: String,
    #[serde(alias = "network.ingress.proxy_buffer_size_kb")]
    pub network_ingress_proxy_buffer_size_kb: u32,
    #[serde(alias = "network.ingress.whitelist_source_range")]
    pub network_ingress_whitelist_source_range: String,
    #[serde(alias = "network.ingress.denylist_source_range")]
    pub network_ingress_denylist_source_range: String,
    #[serde(alias = "network.ingress.basic_auth_env_var")]
    pub network_ingress_basic_auth_env_var: String,
    #[serde(alias = "network.ingress.nginx_controller_server_snippet")]
    pub network_ingress_nginx_controller_server_snippet: Option<NginxServerSnippet>,
    #[serde(alias = "network.ingress.nginx_controller_configuration_snippet")]
    pub network_ingress_nginx_controller_configuration_snippet: Option<NginxConfigurationSnippet>,
    #[serde(alias = "network.ingress.nginx_limit_rpm")]
    pub network_ingress_nginx_limit_rpm: Option<u32>,
    #[serde(alias = "network.ingress.nginx_limit_rps")]
    pub network_ingress_nginx_limit_rps: Option<u32>,
    #[serde(alias = "network.ingress.nginx_limit_burst_multiplier")]
    pub network_ingress_nginx_limit_burst_multiplier: Option<u32>,
    #[serde(alias = "network.ingress.nginx_limit_connections")]
    pub network_ingress_nginx_limit_connections: Option<u32>,
    #[serde(alias = "network.ingress.nginx_custom_http_errors")]
    pub network_ingress_nginx_custom_http_errors: Option<String>,

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
            deployment_lifecycle_post_start_exec_command: vec![],
            deployment_lifecycle_pre_stop_exec_command: vec![],
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
            hpa_cpu_average_utilization_percent: 60,
            hpa_memory_average_utilization_percent: None,
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
    pub min_instances: u32,
    pub max_instances: u32,
    pub public_domain: String,
    pub ports: Vec<Port>,
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
                self.min_instances,
                self.max_instances,
                self.public_domain,
                self.ports,
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
                self.min_instances,
                self.max_instances,
                self.public_domain,
                self.ports,
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
                self.min_instances,
                self.max_instances,
                self.public_domain,
                self.ports,
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
                self.min_instances,
                self.max_instances,
                self.public_domain,
                self.ports,
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
                self.min_instances,
                self.max_instances,
                self.public_domain,
                self.ports,
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
            )?),
        };

        Ok(service)
    }
}

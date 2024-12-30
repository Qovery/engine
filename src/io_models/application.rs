use std::collections::{BTreeMap, BTreeSet};
use std::str;
use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose;
use base64::Engine;
use chrono::{DateTime, Utc};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::engine_task::qovery_api::QoveryApi;
use crate::environment::models;
use crate::environment::models::application::{ApplicationError, ApplicationService};
use crate::environment::models::aws::AwsAppExtraSettings;
use crate::environment::models::gcp::GcpAppExtraSettings;
use crate::environment::models::scaleway::ScwAppExtraSettings;
use crate::environment::models::selfmanaged::OnPremiseAppExtraSettings;
use crate::environment::models::types::{OnPremise, AWS, GCP, SCW};
use crate::infrastructure::models::build_platform::{Build, GitRepository, Image, SshKey};
use crate::infrastructure::models::cloud_provider::io::{NginxConfigurationSnippet, NginxServerSnippet};
use crate::infrastructure::models::cloud_provider::service::ServiceType;
use crate::infrastructure::models::cloud_provider::{CloudProvider, Kind as CPKind};
use crate::infrastructure::models::container_registry::ContainerRegistryInfo;
use crate::io_models::annotations_group::AnnotationsGroup;
use crate::io_models::container::{ContainerAdvancedSettings, Registry};
use crate::io_models::context::Context;
use crate::io_models::labels_group::LabelsGroup;
use crate::io_models::models::{
    CpuArchitecture, EnvironmentVariable, KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit, StorageClass,
};
use crate::io_models::probe::Probe;
use crate::io_models::variable_utils::{default_environment_vars_with_info, VariableInfo};
use crate::io_models::{
    fetch_git_token, normalize_root_and_dockerfile_path, sanitized_git_url, ssh_keys_from_env_vars, Action,
    MountedFile, QoveryIdentifier,
};
use crate::utilities::to_short_id;

use super::{PodAntiAffinity, UpdateStrategy};

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
pub enum Protocol {
    HTTP,
    GRPC,
    TCP,
    UDP,
}

impl Protocol {
    pub fn is_layer4(&self) -> bool {
        matches!(self, Protocol::TCP | Protocol::UDP)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
pub struct Port {
    pub long_id: Uuid,
    pub port: u16,
    pub is_default: bool,
    pub name: String,
    pub publicly_accessible: bool,
    pub protocol: Protocol,
    pub service_name: Option<String>,
    pub namespace: Option<String>,
    pub additional_service: Option<AdditionalService>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
pub struct AdditionalService {
    pub selectors: BTreeMap<String, String>,
}

pub fn to_environment_variable(env_vars: BTreeMap<String, VariableInfo>) -> Vec<EnvironmentVariable> {
    env_vars
        .into_iter()
        .map(|(k, variable_infos)| EnvironmentVariable {
            key: k,
            value: variable_infos.value,
            is_secret: variable_infos.is_secret,
        })
        .collect()
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
pub struct GitCredentials {
    pub login: String,
    pub access_token: String,
    pub expired_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(default)]
pub struct ApplicationAdvancedSettings {
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

    // Build
    #[serde(alias = "build.timeout_max_sec")]
    pub build_timeout_max_sec: u32,
    #[serde(alias = "build.cpu_max_in_milli")]
    pub build_cpu_max_in_milli: u32,
    #[serde(alias = "build.ram_max_in_gib")]
    pub build_ram_max_in_gib: u32,

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
    #[serde(alias = "network.ingress.nginx_limit_burst_multiplier")]
    pub network_ingress_nginx_limit_burst_multiplier: Option<u32>,

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

impl Default for ApplicationAdvancedSettings {
    fn default() -> Self {
        ApplicationAdvancedSettings {
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
            build_timeout_max_sec: 30 * 60,
            build_cpu_max_in_milli: 4000,
            build_ram_max_in_gib: 8,
            network_ingress_proxy_body_size_mb: 100,
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
            network_ingress_nginx_controller_server_snippet: None,
            network_ingress_nginx_controller_configuration_snippet: None,
            network_ingress_nginx_limit_rpm: None,
            network_ingress_nginx_limit_burst_multiplier: None,
            hpa_cpu_average_utilization_percent: 60,
            hpa_memory_average_utilization_percent: None,
        }
    }
}

impl ApplicationAdvancedSettings {
    pub fn to_container_advanced_settings(&self) -> ContainerAdvancedSettings {
        ContainerAdvancedSettings {
            security_service_account_name: self.security_service_account_name.clone(),
            security_read_only_root_filesystem: self.security_read_only_root_filesystem,
            security_automount_service_account_token: self.security_automount_service_account_token,
            deployment_termination_grace_period_seconds: self.deployment_termination_grace_period_seconds,
            deployment_update_strategy_type: self.deployment_update_strategy_type,
            deployment_update_strategy_rolling_update_max_unavailable_percent: self
                .deployment_update_strategy_rolling_update_max_unavailable_percent,
            deployment_update_strategy_rolling_update_max_surge_percent: self
                .deployment_update_strategy_rolling_update_max_surge_percent,
            deployment_affinity_node_required: self.deployment_affinity_node_required.clone(),
            deployment_antiaffinity_pod: self.deployment_antiaffinity_pod.clone(),
            deployment_lifecycle_post_start_exec_command: self.deployment_lifecycle_post_start_exec_command.clone(),
            deployment_lifecycle_pre_stop_exec_command: self.deployment_lifecycle_pre_stop_exec_command.clone(),
            network_ingress_proxy_body_size_mb: self.network_ingress_proxy_body_size_mb,
            network_ingress_cors_enable: self.network_ingress_cors_enable,
            network_ingress_sticky_session_enable: self.network_ingress_sticky_session_enable,
            network_ingress_cors_allow_origin: self.network_ingress_cors_allow_origin.clone(),
            network_ingress_cors_allow_methods: self.network_ingress_cors_allow_methods.clone(),
            network_ingress_cors_allow_headers: self.network_ingress_cors_allow_headers.clone(),
            network_ingress_keepalive_time_seconds: self.network_ingress_keepalive_time_seconds,
            network_ingress_keepalive_timeout_seconds: self.network_ingress_keepalive_timeout_seconds,
            network_ingress_send_timeout_seconds: self.network_ingress_send_timeout_seconds,
            network_ingress_add_headers: self.network_ingress_add_headers.clone(),
            network_ingress_proxy_set_headers: self.network_ingress_proxy_set_headers.clone(),
            network_ingress_proxy_connect_timeout_seconds: self.network_ingress_proxy_connect_timeout_seconds,
            network_ingress_proxy_send_timeout_seconds: self.network_ingress_proxy_send_timeout_seconds,
            network_ingress_proxy_read_timeout_seconds: self.network_ingress_proxy_read_timeout_seconds,
            network_ingress_proxy_request_buffering: self.network_ingress_proxy_request_buffering.clone(),
            network_ingress_proxy_buffering: self.network_ingress_proxy_buffering.clone(),
            network_ingress_proxy_buffer_size_kb: self.network_ingress_proxy_buffer_size_kb,
            network_ingress_whitelist_source_range: self.network_ingress_whitelist_source_range.clone(),
            network_ingress_denylist_source_range: self.network_ingress_denylist_source_range.clone(),
            network_ingress_basic_auth_env_var: self.network_ingress_basic_auth_env_var.clone(),
            network_ingress_grpc_send_timeout_seconds: self.network_ingress_grpc_send_timeout_seconds,
            network_ingress_grpc_read_timeout_seconds: self.network_ingress_grpc_read_timeout_seconds,
            network_ingress_nginx_limit_rpm: self.network_ingress_nginx_limit_rpm,
            network_ingress_nginx_limit_burst_multiplier: self.network_ingress_nginx_limit_burst_multiplier,
            network_ingress_nginx_controller_server_snippet: self
                .network_ingress_nginx_controller_server_snippet
                .clone(),
            network_ingress_nginx_controller_configuration_snippet: self
                .network_ingress_nginx_controller_configuration_snippet
                .clone(),
            hpa_cpu_average_utilization_percent: self.hpa_cpu_average_utilization_percent,
            hpa_memory_average_utilization_percent: self.hpa_memory_average_utilization_percent,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Application {
    pub long_id: Uuid,
    pub name: String,
    pub action: Action,
    pub git_url: String,
    pub git_credentials: Option<GitCredentials>,
    pub kube_name: String,
    pub branch: String,
    pub commit_id: String,
    pub dockerfile_path: Option<String>,
    pub command_args: Vec<String>,
    pub entrypoint: Option<String>,
    #[serde(default = "default_root_path_value")]
    pub root_path: String,
    pub public_domain: String,
    pub ports: Vec<Port>,
    pub cpu_request_in_milli: u32,
    pub cpu_limit_in_milli: u32,
    pub ram_request_in_mib: u32,
    pub ram_limit_in_mib: u32,
    pub min_instances: u32,
    pub max_instances: u32,
    pub storage: Vec<Storage>,
    /// Key is a String, Value is a base64 encoded String
    /// Use BTreeMap to get Hash trait which is not available on HashMap
    #[serde(default = "default_environment_vars_with_info")]
    pub environment_vars_with_infos: BTreeMap<String, VariableInfo>,
    #[serde(default)]
    pub mounted_files: Vec<MountedFile>,
    pub readiness_probe: Option<Probe>,
    pub liveness_probe: Option<Probe>,
    #[serde(default)]
    pub advanced_settings: ApplicationAdvancedSettings,
    pub container_registries: Vec<Registry>,
    #[serde(default)]
    pub annotations_group_ids: BTreeSet<Uuid>,
    #[serde(default)]
    pub labels_group_ids: BTreeSet<Uuid>,
    #[serde(default)] // Default is false
    pub should_delete_shared_registry: bool,
    #[serde(default)] // Default is false
    pub shared_image_feature_enabled: bool,
}

fn default_root_path_value() -> String {
    "/".to_string()
}

impl Application {
    pub fn to_application_domain(
        self,
        context: &Context,
        build: Build,
        cloud_provider: &dyn CloudProvider,
        annotations_group: &BTreeMap<Uuid, AnnotationsGroup>,
        labels_group: &BTreeMap<Uuid, LabelsGroup>,
    ) -> Result<Box<dyn ApplicationService>, ApplicationError> {
        let environment_variables = to_environment_variable(self.environment_vars_with_infos);
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

        match cloud_provider.kind() {
            CPKind::Aws => {
                // Note: we check if kubernetes is EC2 to map to the proper implementation
                // This is far from ideal, it should be checked against an exhaustive match
                // But for the time being, it does the trick since we are already in AWS
                Ok(Box::new(models::application::Application::<AWS>::new(
                    context,
                    self.long_id,
                    self.action.to_service_action(),
                    self.name.as_str(),
                    self.kube_name,
                    self.public_domain,
                    self.ports,
                    self.min_instances,
                    self.max_instances,
                    build,
                    self.command_args,
                    self.entrypoint,
                    self.storage.iter().map(|s| s.to_storage()).collect::<Vec<_>>(),
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
                    KubernetesCpuResourceUnit::MilliCpu(self.cpu_request_in_milli),
                    KubernetesCpuResourceUnit::MilliCpu(self.cpu_limit_in_milli),
                    KubernetesMemoryResourceUnit::MebiByte(self.ram_request_in_mib),
                    KubernetesMemoryResourceUnit::MebiByte(self.ram_limit_in_mib),
                    self.should_delete_shared_registry,
                )?))
            }
            CPKind::Scw => Ok(Box::new(models::application::Application::<SCW>::new(
                context,
                self.long_id,
                self.action.to_service_action(),
                self.name.as_str(),
                self.kube_name,
                self.public_domain,
                self.ports,
                self.min_instances,
                self.max_instances,
                build,
                self.command_args,
                self.entrypoint,
                self.storage.iter().map(|s| s.to_storage()).collect::<Vec<_>>(),
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
                KubernetesCpuResourceUnit::MilliCpu(self.cpu_request_in_milli),
                KubernetesCpuResourceUnit::MilliCpu(self.cpu_limit_in_milli),
                KubernetesMemoryResourceUnit::MebiByte(self.ram_request_in_mib),
                KubernetesMemoryResourceUnit::MebiByte(self.ram_limit_in_mib),
                self.should_delete_shared_registry,
            )?)),
            CPKind::Gcp => Ok(Box::new(models::application::Application::<GCP>::new(
                context,
                self.long_id,
                self.action.to_service_action(),
                self.name.as_str(),
                self.kube_name,
                self.public_domain,
                self.ports,
                self.min_instances,
                self.max_instances,
                build,
                self.command_args,
                self.entrypoint,
                self.storage.iter().map(|s| s.to_storage()).collect::<Vec<_>>(),
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
                KubernetesCpuResourceUnit::MilliCpu(self.cpu_request_in_milli),
                KubernetesCpuResourceUnit::MilliCpu(self.cpu_limit_in_milli),
                KubernetesMemoryResourceUnit::MebiByte(self.ram_request_in_mib),
                KubernetesMemoryResourceUnit::MebiByte(self.ram_limit_in_mib),
                self.should_delete_shared_registry,
            )?)),
            CPKind::OnPremise => Ok(Box::new(models::application::Application::<OnPremise>::new(
                context,
                self.long_id,
                self.action.to_service_action(),
                self.name.as_str(),
                self.kube_name,
                self.public_domain,
                self.ports,
                self.min_instances,
                self.max_instances,
                build,
                self.command_args,
                self.entrypoint,
                self.storage.iter().map(|s| s.to_storage()).collect::<Vec<_>>(),
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
                KubernetesCpuResourceUnit::MilliCpu(self.cpu_request_in_milli),
                KubernetesCpuResourceUnit::MilliCpu(self.cpu_limit_in_milli),
                KubernetesMemoryResourceUnit::MebiByte(self.ram_request_in_mib),
                KubernetesMemoryResourceUnit::MebiByte(self.ram_limit_in_mib),
                self.should_delete_shared_registry,
            )?)),
        }
    }

    fn to_image(&self, cr_info: &ContainerRegistryInfo, cluster_id: &QoveryIdentifier) -> Image {
        Image {
            service_id: to_short_id(&self.long_id),
            service_long_id: self.long_id,
            service_name: self.name.clone(),
            name: match self.shared_image_feature_enabled {
                true => cr_info.get_shared_image_name(cluster_id, sanitized_git_url(self.git_url.as_str())),
                false => cr_info.get_image_name(&self.name),
            },
            tag: "".to_string(), // It needs to be computed after creation
            commit_id: self.commit_id.clone(),
            registry_name: cr_info.registry_name.clone(),
            registry_url: cr_info.endpoint.clone(),
            registry_insecure: cr_info.insecure_registry,
            registry_docker_json_config: cr_info.registry_docker_json_config.clone(),
            repository_name: cr_info.get_repository_name(&self.name),
            shared_repository_name: cr_info
                .get_shared_repository_name(cluster_id, sanitized_git_url(self.git_url.as_str())),
            shared_image_feature_enabled: self.shared_image_feature_enabled,
        }
    }

    pub fn to_build(
        &self,
        registry_url: &ContainerRegistryInfo,
        qovery_api: Arc<dyn QoveryApi>,
        architectures: Vec<CpuArchitecture>,
        cluster_id: &QoveryIdentifier,
    ) -> Build {
        // Get passphrase and public key if provided by the user
        let ssh_keys: Vec<SshKey> = ssh_keys_from_env_vars(&self.environment_vars_with_infos);

        // Convert our root path to an relative path to be able to append them correctly
        let (root_path, dockerfile_path) = normalize_root_and_dockerfile_path(&self.root_path, &self.dockerfile_path);

        //FIXME: Return a result the function
        let url = Url::parse(&self.git_url).unwrap_or_else(|_| Url::parse("https://invalid-git-url.com").unwrap());

        let mut disable_build_cache = false;
        let mut build = Build {
            git_repository: GitRepository {
                url,
                get_credentials: if self.git_credentials.is_none() {
                    None
                } else {
                    let id = self.long_id;
                    Some(Box::new(move || fetch_git_token(&*qovery_api, ServiceType::Application, &id)))
                },
                ssh_keys,
                commit_id: self.commit_id.clone(),
                dockerfile_path,
                dockerfile_content: None,
                root_path,
            },
            image: self.to_image(registry_url, cluster_id),
            environment_variables: self
                .environment_vars_with_infos
                .iter()
                .filter_map(|(k, variable_infos)| {
                    // Remove special vars
                    let v = String::from_utf8(
                        general_purpose::STANDARD
                            .decode(variable_infos.value.as_bytes())
                            .unwrap_or_default(),
                    )
                    .unwrap_or_default();
                    if k == "QOVERY_DISABLE_BUILD_CACHE" && v.to_lowercase() == "true" {
                        disable_build_cache = true;
                        return None;
                    }

                    Some((k.clone(), v))
                })
                .collect::<BTreeMap<_, _>>(),
            disable_cache: disable_build_cache,
            timeout: Duration::from_secs(self.advanced_settings.build_timeout_max_sec as u64),
            architectures,
            max_cpu_in_milli: self.advanced_settings.build_cpu_max_in_milli,
            max_ram_in_gib: self.advanced_settings.build_ram_max_in_gib,
            registries: self.container_registries.clone(),
        };

        build.compute_image_tag();
        build
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Storage {
    pub id: String,
    pub long_id: Uuid,
    pub name: String,
    pub storage_class: String,
    pub size_in_gib: u32,
    pub mount_point: String,
    pub snapshot_retention_in_days: u16,
}

impl Storage {
    pub fn to_storage(&self) -> crate::io_models::models::Storage {
        crate::io_models::models::Storage {
            id: self.id.clone(),
            long_id: self.long_id,
            name: self.name.clone(),
            storage_class: StorageClass(self.storage_class.clone()),
            size_in_gib: self.size_in_gib,
            mount_point: self.mount_point.clone(),
            snapshot_retention_in_days: self.snapshot_retention_in_days,
        }
    }
}

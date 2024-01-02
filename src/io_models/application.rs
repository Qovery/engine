use crate::build_platform::{Build, GitRepository, Image, SshKey};
use crate::cloud_provider::kubernetes::Kind as KubernetesKind;
use crate::cloud_provider::models::{CpuArchitecture, EnvironmentVariable};
use crate::cloud_provider::service::ServiceType;
use crate::cloud_provider::{CloudProvider, Kind as CPKind};
use crate::container_registry::ContainerRegistryInfo;
use crate::engine_task::qovery_api::QoveryApi;
use crate::io_models::container::ContainerAdvancedSettings;
use crate::io_models::context::Context;
use crate::io_models::probe::Probe;
use crate::io_models::variable_utils::{default_environment_vars_with_info, VariableInfo};
use crate::io_models::{
    fetch_git_token, normalize_root_and_dockerfile_path, ssh_keys_from_env_vars, Action, MountedFile,
};
use crate::models;
use crate::models::application::{ApplicationError, ApplicationService};
use crate::models::aws::{AwsAppExtraSettings, AwsStorageType};
use crate::models::aws_ec2::{AwsEc2AppExtraSettings, AwsEc2StorageType};
use crate::models::gcp::{GcpAppExtraSettings, GcpStorageType};
use crate::models::scaleway::{ScwAppExtraSettings, ScwStorageType};
use crate::models::selfmanaged::SelfManagedAppExtraSettings;
use crate::models::types::{AWSEc2, SelfManaged, AWS, GCP, SCW};
use crate::utilities::to_short_id;
use base64::engine::general_purpose;
use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::str;
use std::sync::Arc;
use std::time::Duration;
use url::Url;
use uuid::Uuid;

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

    // Deployment
    #[serde(alias = "deployment.termination_grace_period_seconds")]
    pub deployment_termination_grace_period_seconds: u32,
    #[serde(alias = "deployment.custom_domain_check_enabled")]
    pub deployment_custom_domain_check_enabled: bool,
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
    #[serde(alias = "network.ingress.extra_headers")]
    pub network_ingress_extra_headers: BTreeMap<String, String>,
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

    #[serde(alias = "network.ingress.grpc_send_timeout_seconds")]
    pub network_ingress_grpc_send_timeout_seconds: u32,
    #[serde(alias = "network.ingress.grpc_read_timeout_seconds")]
    pub network_ingress_grpc_read_timeout_seconds: u32,

    // Pod autoscaler
    #[serde(alias = "hpa.cpu.average_utilization_percent")]
    pub hpa_cpu_average_utilization_percent: u8,
}

impl Default for ApplicationAdvancedSettings {
    fn default() -> Self {
        ApplicationAdvancedSettings {
            security_service_account_name: "".to_string(),
            security_read_only_root_filesystem: false,
            deployment_termination_grace_period_seconds: 60,
            deployment_custom_domain_check_enabled: true,
            deployment_update_strategy_type: UpdateStrategy::RollingUpdate,
            deployment_update_strategy_rolling_update_max_unavailable_percent: 25,
            deployment_update_strategy_rolling_update_max_surge_percent: 25,
            deployment_affinity_node_required: BTreeMap::new(),
            deployment_antiaffinity_pod: PodAntiAffinity::Preferred,
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
            network_ingress_extra_headers: BTreeMap::new(),
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
            hpa_cpu_average_utilization_percent: 60,
        }
    }
}

impl ApplicationAdvancedSettings {
    pub fn to_container_advanced_settings(&self) -> ContainerAdvancedSettings {
        ContainerAdvancedSettings {
            security_service_account_name: self.security_service_account_name.clone(),
            security_read_only_root_filesystem: self.security_read_only_root_filesystem,
            deployment_custom_domain_check_enabled: self.deployment_custom_domain_check_enabled,
            deployment_termination_grace_period_seconds: self.deployment_termination_grace_period_seconds,
            deployment_update_strategy_type: self.deployment_update_strategy_type,
            deployment_update_strategy_rolling_update_max_unavailable_percent: self
                .deployment_update_strategy_rolling_update_max_unavailable_percent,
            deployment_update_strategy_rolling_update_max_surge_percent: self
                .deployment_update_strategy_rolling_update_max_surge_percent,
            deployment_affinity_node_required: self.deployment_affinity_node_required.clone(),
            deployment_antiaffinity_pod: self.deployment_antiaffinity_pod.clone(),
            network_ingress_proxy_body_size_mb: self.network_ingress_proxy_body_size_mb,
            network_ingress_cors_enable: self.network_ingress_cors_enable,
            network_ingress_sticky_session_enable: self.network_ingress_sticky_session_enable,
            network_ingress_cors_allow_origin: self.network_ingress_cors_allow_origin.clone(),
            network_ingress_cors_allow_methods: self.network_ingress_cors_allow_methods.clone(),
            network_ingress_cors_allow_headers: self.network_ingress_cors_allow_headers.clone(),
            network_ingress_keepalive_time_seconds: self.network_ingress_keepalive_time_seconds,
            network_ingress_keepalive_timeout_seconds: self.network_ingress_keepalive_timeout_seconds,
            network_ingress_send_timeout_seconds: self.network_ingress_send_timeout_seconds,
            network_ingress_extra_headers: self.network_ingress_extra_headers.clone(),
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
            hpa_cpu_average_utilization_percent: self.hpa_cpu_average_utilization_percent,
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
    pub buildpack_language: Option<String>,
    #[serde(default = "default_root_path_value")]
    pub root_path: String,
    pub public_domain: String,
    pub ports: Vec<Port>,
    pub total_cpus: String,
    pub cpu_burst: String,
    pub total_ram_in_mib: u32,
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
    ) -> Result<Box<dyn ApplicationService>, ApplicationError> {
        let environment_variables = to_environment_variable(self.environment_vars_with_infos);

        match cloud_provider.kind() {
            CPKind::Aws => {
                // Note: we check if kubernetes is EC2 to map to the proper implementation
                // This is far from ideal, it should be checked against an exhaustive match
                // But for the time being, it does the trick since we are already in AWS
                if cloud_provider.kubernetes_kind() == KubernetesKind::Eks {
                    Ok(Box::new(models::application::Application::<AWS>::new(
                        context,
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        self.kube_name,
                        self.public_domain,
                        self.ports,
                        self.total_cpus,
                        self.cpu_burst,
                        self.total_ram_in_mib,
                        self.min_instances,
                        self.max_instances,
                        build,
                        self.command_args,
                        self.entrypoint,
                        self.storage.iter().map(|s| s.to_aws_storage()).collect::<Vec<_>>(),
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
                    )?))
                } else {
                    Ok(Box::new(models::application::Application::<AWSEc2>::new(
                        context,
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        self.kube_name,
                        self.public_domain,
                        self.ports,
                        self.total_cpus,
                        self.cpu_burst,
                        self.total_ram_in_mib,
                        self.min_instances,
                        self.max_instances,
                        build,
                        self.command_args,
                        self.entrypoint,
                        self.storage.iter().map(|s| s.to_aws_ec2_storage()).collect::<Vec<_>>(),
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
                    )?))
                }
            }
            CPKind::Scw => Ok(Box::new(models::application::Application::<SCW>::new(
                context,
                self.long_id,
                self.action.to_service_action(),
                self.name.as_str(),
                self.kube_name,
                self.public_domain,
                self.ports,
                self.total_cpus,
                self.cpu_burst,
                self.total_ram_in_mib,
                self.min_instances,
                self.max_instances,
                build,
                self.command_args,
                self.entrypoint,
                self.storage.iter().map(|s| s.to_scw_storage()).collect::<Vec<_>>(),
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
            )?)),
            CPKind::Gcp => Ok(Box::new(models::application::Application::<GCP>::new(
                context,
                self.long_id,
                self.action.to_service_action(),
                self.name.as_str(),
                self.kube_name,
                self.public_domain,
                self.ports,
                self.total_cpus,
                self.cpu_burst,
                self.total_ram_in_mib,
                self.min_instances,
                self.max_instances,
                build,
                self.command_args,
                self.entrypoint,
                self.storage.iter().map(|s| s.to_gcp_storage()).collect::<Vec<_>>(),
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
            )?)),
            CPKind::SelfManaged => Ok(Box::new(models::application::Application::<SelfManaged>::new(
                context,
                self.long_id,
                self.action.to_service_action(),
                self.name.as_str(),
                self.kube_name,
                self.public_domain,
                self.ports,
                self.total_cpus,
                self.cpu_burst,
                self.total_ram_in_mib,
                self.min_instances,
                self.max_instances,
                build,
                self.command_args,
                self.entrypoint,
                vec![],
                environment_variables,
                self.mounted_files
                    .iter()
                    .map(|e| e.to_domain())
                    .collect::<BTreeSet<_>>(),
                self.readiness_probe.map(|p| p.to_domain()),
                self.liveness_probe.map(|p| p.to_domain()),
                self.advanced_settings,
                SelfManagedAppExtraSettings {},
                |transmitter| context.get_event_details(transmitter),
            )?)),
        }
    }

    fn to_image(&self, cr_info: &ContainerRegistryInfo) -> Image {
        Image {
            service_id: to_short_id(&self.long_id),
            service_long_id: self.long_id,
            service_name: self.name.clone(),
            name: (cr_info.get_image_name)(&self.name),
            tag: "".to_string(), // It needs to be compute after creation
            commit_id: self.commit_id.clone(),
            registry_name: cr_info.registry_name.clone(),
            registry_url: cr_info.endpoint.clone(),
            registry_docker_json_config: cr_info.registry_docker_json_config.clone(),
            repository_name: (cr_info.get_repository_name)(&self.name),
        }
    }

    pub fn to_build(
        &self,
        registry_url: &ContainerRegistryInfo,
        qovery_api: Arc<dyn QoveryApi>,
        architectures: Vec<CpuArchitecture>,
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
                root_path,
                buildpack_language: self.buildpack_language.clone(),
            },
            image: self.to_image(registry_url),
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
    pub storage_type: StorageType,
    pub size_in_gib: u32,
    pub mount_point: String,
    pub snapshot_retention_in_days: u16,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StorageType {
    SlowHdd,
    Hdd,
    Ssd,
    FastSsd,
}

impl Storage {
    pub fn to_aws_storage(&self) -> crate::cloud_provider::models::Storage<AwsStorageType> {
        crate::cloud_provider::models::Storage {
            id: self.id.clone(),
            long_id: self.long_id,
            name: self.name.clone(),
            storage_type: match self.storage_type {
                StorageType::SlowHdd => AwsStorageType::SC1,
                StorageType::Hdd => AwsStorageType::ST1,
                StorageType::Ssd => AwsStorageType::GP2,
                StorageType::FastSsd => AwsStorageType::IO1,
            },
            size_in_gib: self.size_in_gib,
            mount_point: self.mount_point.clone(),
            snapshot_retention_in_days: self.snapshot_retention_in_days,
        }
    }

    pub fn to_aws_ec2_storage(&self) -> crate::cloud_provider::models::Storage<AwsEc2StorageType> {
        crate::cloud_provider::models::Storage {
            id: self.id.clone(),
            long_id: self.long_id,
            name: self.name.clone(),
            storage_type: match self.storage_type {
                StorageType::SlowHdd => AwsEc2StorageType::SC1,
                StorageType::Hdd => AwsEc2StorageType::ST1,
                StorageType::Ssd => AwsEc2StorageType::GP2,
                StorageType::FastSsd => AwsEc2StorageType::IO1,
            },
            size_in_gib: self.size_in_gib,
            mount_point: self.mount_point.clone(),
            snapshot_retention_in_days: self.snapshot_retention_in_days,
        }
    }

    pub fn to_scw_storage(&self) -> crate::cloud_provider::models::Storage<ScwStorageType> {
        crate::cloud_provider::models::Storage {
            id: self.id.clone(),
            long_id: self.long_id,
            name: self.name.clone(),
            storage_type: ScwStorageType::BlockSsd,
            size_in_gib: self.size_in_gib,
            mount_point: self.mount_point.clone(),
            snapshot_retention_in_days: self.snapshot_retention_in_days,
        }
    }

    pub fn to_gcp_storage(&self) -> crate::cloud_provider::models::Storage<GcpStorageType> {
        crate::cloud_provider::models::Storage {
            id: self.id.clone(),
            long_id: self.long_id,
            name: self.name.clone(),
            storage_type: match self.storage_type {
                StorageType::SlowHdd => GcpStorageType::Standard,
                StorageType::Hdd => GcpStorageType::Balanced,
                StorageType::Ssd => GcpStorageType::SSD,
                StorageType::FastSsd => GcpStorageType::Extreme,
            },
            size_in_gib: self.size_in_gib,
            mount_point: self.mount_point.clone(),
            snapshot_retention_in_days: self.snapshot_retention_in_days,
        }
    }
}

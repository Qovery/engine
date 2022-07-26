use crate::build_platform::{Build, Credentials, GitRepository, Image, SshKey};
use crate::cloud_provider::kubernetes::Kind as KubernetesKind;
use crate::cloud_provider::{CloudProvider, Kind as CPKind};
use crate::container_registry::ContainerRegistryInfo;
use crate::io_models::context::Context;
use crate::io_models::Action;
use crate::logger::Logger;
use crate::models;
use crate::models::application::{ApplicationError, ApplicationService};
use crate::models::aws::{AwsAppExtraSettings, AwsStorageType};
use crate::models::aws_ec2::{AwsEc2AppExtraSettings, AwsEc2StorageType};
use crate::models::digital_ocean::{DoAppExtraSettings, DoStorageType};
use crate::models::scaleway::{ScwAppExtraSettings, ScwStorageType};
use crate::models::types::{AWSEc2, AWS, DO, SCW};
use crate::utilities::to_short_id;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use url::Url;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub enum Protocol {
    HTTP,
    TCP,
    UDP,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Port {
    pub id: String,
    pub long_id: Uuid,
    pub port: u16,
    pub public_port: Option<u16>,
    pub name: Option<String>,
    pub publicly_accessible: bool,
    pub protocol: Protocol,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ApplicationAdvancedSettingsProbeType {
    None,
    Tcp,
    Http,
}

fn to_environment_variable(
    env_vars: &BTreeMap<String, String>,
) -> Vec<crate::cloud_provider::models::EnvironmentVariable> {
    env_vars
        .iter()
        .map(|(k, v)| crate::cloud_provider::models::EnvironmentVariable {
            key: k.clone(),
            value: v.clone(),
        })
        .collect()
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct GitCredentials {
    pub login: String,
    pub access_token: String,
    pub expired_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(default)]
pub struct ApplicationAdvancedSettings {
    #[deprecated(
        note = "please use `readiness_probe.initial_delay_seconds` and `liveness_probe.initial_delay_seconds` instead"
    )]
    #[serde(alias = "deployment.delay_start_time_sec")]
    pub deployment_delay_start_time_sec: u32,
    #[serde(alias = "deployment.custom_domain_check_enabled")]
    pub deployment_custom_domain_check_enabled: bool,
    #[serde(alias = "build.timeout_max_sec")]
    pub build_timeout_max_sec: u32,
    #[serde(alias = "network.ingress.proxy_body_size_mb")]
    pub network_ingress_proxy_body_size_mb: u32,
    #[serde(alias = "network.ingress.cors_enable")]
    pub network_ingress_cors_enable: bool,
    #[serde(alias = "network.ingress.cors_allow_origin")]
    pub network_ingress_cors_allow_origin: String,
    #[serde(alias = "network.ingress.cors_methods")]
    pub network_ingress_cors_allow_methods: String,
    #[serde(alias = "network.ingress.cors_allowed_headers")]
    pub network_ingress_cors_allow_headers: String,
    #[serde(alias = "readiness_probe.type")]
    pub readiness_probe_type: ApplicationAdvancedSettingsProbeType,
    #[serde(alias = "readiness_probe.http_get.path")]
    pub readiness_probe_http_get_path: String,
    #[serde(alias = "readiness_probe.initial_delay_seconds")]
    pub readiness_probe_initial_delay_seconds: u32,
    #[serde(alias = "readiness_probe.period_seconds")]
    pub readiness_probe_period_seconds: u32,
    #[serde(alias = "readiness_probe.timeout_seconds")]
    pub readiness_probe_timeout_seconds: u32,
    #[serde(alias = "readiness_probe.success_threshold")]
    pub readiness_probe_success_threshold: u32,
    #[serde(alias = "readiness_probe.failure_threshold")]
    pub readiness_probe_failure_threshold: u32,
    #[serde(alias = "liveness_probe.type")]
    pub liveness_probe_type: ApplicationAdvancedSettingsProbeType,
    #[serde(alias = "liveness_probe.http_get.path")]
    pub liveness_probe_http_get_path: String,
    #[serde(alias = "liveness_probe.initial_delay_seconds")]
    pub liveness_probe_initial_delay_seconds: u32,
    #[serde(alias = "liveness_probe.period_seconds")]
    pub liveness_probe_period_seconds: u32,
    #[serde(alias = "liveness_probe.timeout_seconds")]
    pub liveness_probe_timeout_seconds: u32,
    #[serde(alias = "liveness_probe.success_threshold")]
    pub liveness_probe_success_threshold: u32,
    #[serde(alias = "liveness_probe.failure_threshold")]
    pub liveness_probe_failure_threshold: u32,
    #[serde(alias = "hpa.cpu.average_utilization_percent")]
    pub hpa_cpu_average_utilization_percent: i8,
}

impl Default for ApplicationAdvancedSettings {
    fn default() -> Self {
        ApplicationAdvancedSettings {
            deployment_delay_start_time_sec: 30,
            build_timeout_max_sec: 30 * 60, // 30min
            deployment_custom_domain_check_enabled: true,
            network_ingress_proxy_body_size_mb: 100,
            network_ingress_cors_enable: false,
            network_ingress_cors_allow_origin: "*".to_string(),
            network_ingress_cors_allow_methods: "GET, PUT, POST, DELETE, PATCH, OPTIONS".to_string(),
            network_ingress_cors_allow_headers: "DNT,Keep-Alive,User-Agent,X-Requested-With,If-Modified-Since,Cache-Control,Content-Type,Range,Authorization".to_string(),
            readiness_probe_type: ApplicationAdvancedSettingsProbeType::Tcp,
            readiness_probe_http_get_path: "/".to_string(),
            readiness_probe_initial_delay_seconds: 30,
            readiness_probe_period_seconds: 10,
            readiness_probe_timeout_seconds: 1,
            readiness_probe_success_threshold: 1,
            readiness_probe_failure_threshold: 3,
            liveness_probe_type: ApplicationAdvancedSettingsProbeType::Tcp,
            liveness_probe_http_get_path: "/".to_string(),
            liveness_probe_initial_delay_seconds: 30,
            liveness_probe_period_seconds: 10,
            liveness_probe_timeout_seconds: 5,
            liveness_probe_success_threshold: 1,
            liveness_probe_failure_threshold: 3,
            hpa_cpu_average_utilization_percent: 60,
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
    pub branch: String,
    pub commit_id: String,
    pub dockerfile_path: Option<String>,
    pub buildpack_language: Option<String>,
    #[serde(default = "default_root_path_value")]
    pub root_path: String,
    pub ports: Vec<Port>,
    pub total_cpus: String,
    pub cpu_burst: String,
    pub total_ram_in_mib: u32,
    pub min_instances: u32,
    pub max_instances: u32,
    pub storage: Vec<Storage>,
    /// Key is a String, Value is a base64 encoded String
    /// Use BTreeMap to get Hash trait which is not available on HashMap
    pub environment_vars: BTreeMap<String, String>,
    #[serde(default)]
    pub advanced_settings: ApplicationAdvancedSettings,
}

fn default_root_path_value() -> String {
    "/".to_string()
}

impl Application {
    pub fn to_application_domain(
        &self,
        context: &Context,
        build: Build,
        cloud_provider: &dyn CloudProvider,
        logger: Box<dyn Logger>,
    ) -> Result<Box<dyn ApplicationService>, ApplicationError> {
        let environment_variables = to_environment_variable(&self.environment_vars);
        let listeners = cloud_provider.listeners().clone();

        match cloud_provider.kind() {
            CPKind::Aws => {
                // Note: we check if kubernetes is EC2 to map to the proper implementation
                // This is far from ideal, it should be checked against an exhaustive match
                // But for the time being, it does the trick since we are already in AWS
                if cloud_provider.kubernetes_kind() == KubernetesKind::Eks {
                    Ok(Box::new(models::application::Application::<AWS>::new(
                        context.clone(),
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        self.ports.clone(),
                        self.total_cpus.clone(),
                        self.cpu_burst.clone(),
                        self.total_ram_in_mib,
                        self.min_instances,
                        self.max_instances,
                        build,
                        self.storage.iter().map(|s| s.to_aws_storage()).collect::<Vec<_>>(),
                        environment_variables,
                        self.advanced_settings.clone(),
                        AwsAppExtraSettings {},
                        listeners,
                        logger.clone(),
                    )?))
                } else {
                    Ok(Box::new(models::application::Application::<AWSEc2>::new(
                        context.clone(),
                        self.long_id,
                        self.action.to_service_action(),
                        self.name.as_str(),
                        self.ports.clone(),
                        self.total_cpus.clone(),
                        self.cpu_burst.clone(),
                        self.total_ram_in_mib,
                        self.min_instances,
                        self.max_instances,
                        build,
                        self.storage.iter().map(|s| s.to_aws_ec2_storage()).collect::<Vec<_>>(),
                        environment_variables,
                        self.advanced_settings.clone(),
                        AwsEc2AppExtraSettings {},
                        listeners,
                        logger.clone(),
                    )?))
                }
            }
            CPKind::Do => Ok(Box::new(models::application::Application::<DO>::new(
                context.clone(),
                self.long_id,
                self.action.to_service_action(),
                self.name.as_str(),
                self.ports.clone(),
                self.total_cpus.clone(),
                self.cpu_burst.clone(),
                self.total_ram_in_mib,
                self.min_instances,
                self.max_instances,
                build,
                self.storage.iter().map(|s| s.to_do_storage()).collect::<Vec<_>>(),
                environment_variables,
                self.advanced_settings.clone(),
                DoAppExtraSettings {},
                listeners,
                logger.clone(),
            )?)),
            CPKind::Scw => Ok(Box::new(models::application::Application::<SCW>::new(
                context.clone(),
                self.long_id,
                self.action.to_service_action(),
                self.name.as_str(),
                self.ports.clone(),
                self.total_cpus.clone(),
                self.cpu_burst.clone(),
                self.total_ram_in_mib,
                self.min_instances,
                self.max_instances,
                build,
                self.storage.iter().map(|s| s.to_scw_storage()).collect::<Vec<_>>(),
                environment_variables,
                self.advanced_settings.clone(),
                ScwAppExtraSettings {},
                listeners,
                logger.clone(),
            )?)),
        }
    }

    fn to_image(&self, cr_info: &ContainerRegistryInfo) -> Image {
        Image {
            application_id: to_short_id(&self.long_id),
            name: (cr_info.get_image_name)(&self.name),
            tag: "".to_string(), // It needs to be compute after creation
            commit_id: self.commit_id.clone(),
            registry_name: cr_info.registry_name.clone(),
            registry_url: cr_info.endpoint.clone(),
            registry_docker_json_config: cr_info.registry_docker_json_config.clone(),
            repository_name: (cr_info.get_repository_name)(&self.name),
        }
    }

    pub fn to_build(&self, registry_url: &ContainerRegistryInfo) -> Build {
        // Retrieve ssh keys from env variables
        const ENV_GIT_PREFIX: &str = "GIT_SSH_KEY";
        let env_ssh_keys: Vec<(String, String)> = self
            .environment_vars
            .iter()
            .filter_map(|(name, value)| {
                if name.starts_with(ENV_GIT_PREFIX) {
                    Some((name.clone(), value.clone()))
                } else {
                    None
                }
            })
            .collect();

        // Get passphrase and public key if provided by the user
        let mut ssh_keys: Vec<SshKey> = Vec::with_capacity(env_ssh_keys.len());
        for (ssh_key_name, private_key) in env_ssh_keys {
            let private_key = if let Ok(Ok(private_key)) = base64::decode(private_key).map(String::from_utf8) {
                private_key
            } else {
                error!("Invalid base64 environment variable for {}", ssh_key_name);
                continue;
            };

            let passphrase = self
                .environment_vars
                .get(&ssh_key_name.replace(ENV_GIT_PREFIX, "GIT_SSH_PASSPHRASE"))
                .and_then(|val| base64::decode(val).ok())
                .and_then(|str| String::from_utf8(str).ok());

            let public_key = self
                .environment_vars
                .get(&ssh_key_name.replace(ENV_GIT_PREFIX, "GIT_SSH_PUBLIC_KEY"))
                .and_then(|val| base64::decode(val).ok())
                .and_then(|str| String::from_utf8(str).ok());

            ssh_keys.push(SshKey {
                private_key,
                passphrase,
                public_key,
            });
        }

        // Convert our root path to an relative path to be able to append them correctly
        let root_path = if Path::new(&self.root_path).is_absolute() {
            PathBuf::from(self.root_path.trim_start_matches('/'))
        } else {
            PathBuf::from(&self.root_path)
        };
        assert!(root_path.is_relative(), "root path is not a relative path");

        let dockerfile_path = self.dockerfile_path.as_ref().map(|path| {
            if Path::new(&path).is_absolute() {
                root_path.join(path.trim_start_matches('/'))
            } else {
                root_path.join(&path)
            }
        });

        //FIXME: Return a result the function
        let url = Url::parse(&self.git_url).unwrap_or_else(|_| Url::parse("https://invalid-git-url.com").unwrap());

        let mut disable_build_cache = false;
        let mut build = Build {
            git_repository: GitRepository {
                url,
                credentials: self.git_credentials.as_ref().map(|credentials| Credentials {
                    login: credentials.login.clone(),
                    password: credentials.access_token.clone(),
                }),
                ssh_keys,
                commit_id: self.commit_id.clone(),
                dockerfile_path,
                root_path,
                buildpack_language: self.buildpack_language.clone(),
            },
            image: self.to_image(registry_url),
            environment_variables: self
                .environment_vars
                .iter()
                .filter_map(|(k, v)| {
                    // Remove special vars
                    let v = String::from_utf8_lossy(&base64::decode(v.as_bytes()).unwrap_or_default()).into_owned();
                    if k == "QOVERY_DISABLE_BUILD_CACHE" && v.to_lowercase() == "true" {
                        disable_build_cache = true;
                        return None;
                    }

                    Some((k.clone(), v))
                })
                .collect::<BTreeMap<_, _>>(),
            disable_cache: disable_build_cache,
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
    pub size_in_gib: u16,
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

    pub fn to_do_storage(&self) -> crate::cloud_provider::models::Storage<DoStorageType> {
        crate::cloud_provider::models::Storage {
            id: self.id.clone(),
            name: self.name.clone(),
            storage_type: DoStorageType::Standard,
            size_in_gib: self.size_in_gib,
            mount_point: self.mount_point.clone(),
            snapshot_retention_in_days: self.snapshot_retention_in_days,
        }
    }

    pub fn to_scw_storage(&self) -> crate::cloud_provider::models::Storage<ScwStorageType> {
        crate::cloud_provider::models::Storage {
            id: self.id.clone(),
            name: self.name.clone(),
            storage_type: ScwStorageType::BlockSsd,
            size_in_gib: self.size_in_gib,
            mount_point: self.mount_point.clone(),
            snapshot_retention_in_days: self.snapshot_retention_in_days,
        }
    }
}

use crate::build_platform::{Build, Credentials, GitRepository, Image, SshKey};
use crate::cloud_provider::kubernetes::Kind as KubernetesKind;
use crate::cloud_provider::{CloudProvider, Kind};
use crate::container_registry::{ContainerRegistry, ContainerRegistryInfo};
use crate::io_models::application::{to_environment_variable, AdvancedSettingsProbeType, GitCredentials};
use crate::io_models::container::Registry;
use crate::io_models::context::Context;
use crate::io_models::{normalize_root_and_dockerfile_path, ssh_keys_from_env_vars, Action};
use crate::models;
use crate::models::aws::AwsAppExtraSettings;
use crate::models::aws_ec2::AwsEc2AppExtraSettings;
use crate::models::job::{ImageSource, JobError, JobService, RegistryImageSource};
use crate::models::scaleway::ScwAppExtraSettings;
use crate::models::types::{AWSEc2, AWS, SCW};
use crate::utilities::to_short_id;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;
use url::Url;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
#[serde(default)]
pub struct JobAdvancedSettings {
    // Job specific
    #[serde(alias = "job.delete_ttl_seconds_after_finished")]
    pub job_delete_ttl_seconds_after_finished: Option<u32>,

    #[serde(alias = "cronjob.concurrency_policy")]
    pub cronjob_concurrency_policy: String,

    #[serde(alias = "cronjob.failed_jobs_history_limit")]
    pub cronjob_failed_jobs_history_limit: u32,

    #[serde(alias = "cronjob.success_jobs_history_limit")]
    pub cronjob_success_jobs_history_limit: u32,

    // Build
    #[serde(alias = "build.timeout_max_sec")]
    pub build_timeout_max_sec: u32,

    #[serde(alias = "security.service_account_name")]
    pub security_service_account_name: String,

    // Readiness Probes
    #[serde(alias = "readiness_probe.type")]
    pub readiness_probe_type: AdvancedSettingsProbeType,
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

    // Liveness Probes
    #[serde(alias = "liveness_probe.type")]
    pub liveness_probe_type: AdvancedSettingsProbeType,
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
}

impl Default for JobAdvancedSettings {
    fn default() -> Self {
        Self {
            job_delete_ttl_seconds_after_finished: None,
            cronjob_concurrency_policy: "Forbid".to_string(),
            cronjob_failed_jobs_history_limit: 1,
            cronjob_success_jobs_history_limit: 1,
            build_timeout_max_sec: 30 * 60, // 30 minutes
            security_service_account_name: "".to_string(),
            readiness_probe_type: AdvancedSettingsProbeType::None,
            readiness_probe_http_get_path: "".to_string(),
            readiness_probe_initial_delay_seconds: 0,
            readiness_probe_period_seconds: 0,
            readiness_probe_timeout_seconds: 0,
            readiness_probe_success_threshold: 0,
            readiness_probe_failure_threshold: 0,
            liveness_probe_type: AdvancedSettingsProbeType::None,
            liveness_probe_http_get_path: "".to_string(),
            liveness_probe_initial_delay_seconds: 0,
            liveness_probe_period_seconds: 0,
            liveness_probe_timeout_seconds: 0,
            liveness_probe_success_threshold: 0,
            liveness_probe_failure_threshold: 0,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum JobSchedule {
    OnStart {},
    OnPause {},
    OnDelete {},
    Cron { schedule: String },
}
impl JobSchedule {
    pub fn is_cronjob(&self) -> bool {
        matches!(self, JobSchedule::Cron { .. })
    }

    pub fn is_job(&self) -> bool {
        !self.is_cronjob()
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum JobSource {
    Image {
        registry: Registry,
        image: String,
        tag: String,
    },
    Docker {
        git_url: String,
        git_credentials: Option<GitCredentials>,
        branch: String,
        commit_id: String,
        dockerfile_path: Option<String>,
        root_path: String,
    },
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Job {
    pub long_id: Uuid,
    pub name: String,
    pub action: Action,
    pub schedule: JobSchedule,
    pub source: JobSource,
    pub max_nb_restart: u32,       // .spec.backoffLimit
    pub max_duration_in_sec: u64,  // .spec.activeDeadlineSeconds
    pub default_port: Option<u16>, // for probes
    pub command_args: Vec<String>,
    pub entrypoint: Option<String>,
    pub force_trigger: bool,
    pub cpu_request_in_milli: u32,
    pub cpu_limit_in_milli: u32,
    pub ram_request_in_mib: u32,
    pub ram_limit_in_mib: u32,
    /// Key is a String, Value is a base64 encoded String
    /// Use BTreeMap to get Hash trait which is not available on HashMap
    pub environment_vars: BTreeMap<String, String>,
    #[serde(default)]
    pub advanced_settings: JobAdvancedSettings,
}

impl Job {
    pub fn to_build(&self, registry_url: &ContainerRegistryInfo) -> Option<Build> {
        let (git_url, git_credentials, _branch, commit_id, dockerfile_path, root_path) = match &self.source {
            JobSource::Docker {
                git_url,
                git_credentials,
                branch,
                commit_id,
                dockerfile_path,
                root_path,
            } => (git_url, git_credentials, branch, commit_id, dockerfile_path, root_path),
            _ => return None,
        };

        // Retrieve ssh keys from env variables

        // Get passphrase and public key if provided by the user
        let ssh_keys: Vec<SshKey> = ssh_keys_from_env_vars(&self.environment_vars);

        // Convert our root path to an relative path to be able to append them correctly
        let (root_path, dockerfile_path) = normalize_root_and_dockerfile_path(root_path, dockerfile_path);

        let url = Url::parse(git_url).unwrap_or_else(|_| {
            Url::parse("https://invalid-git-url.com").expect("Error while trying to parse invalid git url")
        });

        let mut disable_build_cache = false;
        let mut build = Build {
            git_repository: GitRepository {
                url,
                credentials: git_credentials.as_ref().map(|credentials| Credentials {
                    login: credentials.login.clone(),
                    password: credentials.access_token.clone(),
                }),
                ssh_keys,
                commit_id: commit_id.clone(),
                dockerfile_path,
                root_path,
                buildpack_language: None,
            },
            image: self.to_image(commit_id.to_string(), registry_url),
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
            timeout: Duration::from_secs(self.advanced_settings.build_timeout_max_sec as u64),
        };

        build.compute_image_tag();
        Some(build)
    }

    fn to_image(&self, commit_id: String, cr_info: &ContainerRegistryInfo) -> Image {
        Image {
            application_id: to_short_id(&self.long_id),
            application_long_id: self.long_id,
            application_name: self.name.clone(),
            name: (cr_info.get_image_name)(&self.long_id.to_string()),
            tag: "".to_string(), // It needs to be compute after creation
            commit_id,
            registry_name: cr_info.registry_name.clone(),
            registry_url: cr_info.endpoint.clone(),
            registry_docker_json_config: cr_info.registry_docker_json_config.clone(),
            repository_name: (cr_info.get_repository_name)(&self.long_id.to_string()),
        }
    }

    pub fn to_job_domain(
        self,
        context: &Context,
        cloud_provider: &dyn CloudProvider,
        default_container_registry: &dyn ContainerRegistry,
    ) -> Result<Box<dyn JobService>, JobError> {
        let image_source = match self.source {
            JobSource::Docker { .. } => {
                let build = match self.to_build(default_container_registry.registry_info()) {
                    Some(build) => Ok(build),
                    None => Err(JobError::InvalidConfig(
                        "Cannot convert docker JobSoure to Build source".to_string(),
                    )),
                }?;

                ImageSource::Build {
                    source: Box::new(build),
                }
            }
            JobSource::Image {
                mut registry,
                image,
                tag,
            } => {
                // Default registry is a bit special as the core does not knows its url/credentials as it is retrieved by us with some tags
                if registry.id() == default_container_registry.long_id() {
                    registry.set_url(default_container_registry.registry_info().endpoint.clone());
                }
                ImageSource::Registry {
                    source: Box::new(RegistryImageSource { registry, image, tag }),
                }
            }
        };

        let environment_variables = to_environment_variable(self.environment_vars);

        let service: Box<dyn JobService> = match cloud_provider.kind() {
            Kind::Aws => {
                if cloud_provider.kubernetes_kind() == KubernetesKind::Eks {
                    Box::new(models::job::Job::<AWS>::new(
                        context,
                        self.long_id,
                        self.name,
                        self.action.to_service_action(),
                        image_source,
                        self.schedule,
                        self.max_nb_restart,
                        Duration::from_secs(self.max_duration_in_sec),
                        self.default_port,
                        self.command_args,
                        self.entrypoint,
                        self.force_trigger,
                        self.cpu_request_in_milli,
                        self.cpu_limit_in_milli,
                        self.ram_request_in_mib,
                        self.ram_limit_in_mib,
                        environment_variables,
                        self.advanced_settings,
                        AwsAppExtraSettings {},
                        |transmitter| context.get_event_details(transmitter),
                    )?)
                } else {
                    Box::new(models::job::Job::<AWSEc2>::new(
                        context,
                        self.long_id,
                        self.name,
                        self.action.to_service_action(),
                        image_source,
                        self.schedule,
                        self.max_nb_restart,
                        Duration::from_secs(self.max_duration_in_sec),
                        self.default_port,
                        self.command_args,
                        self.entrypoint,
                        self.force_trigger,
                        self.cpu_request_in_milli,
                        self.cpu_limit_in_milli,
                        self.ram_request_in_mib,
                        self.ram_limit_in_mib,
                        environment_variables,
                        self.advanced_settings,
                        AwsEc2AppExtraSettings {},
                        |transmitter| context.get_event_details(transmitter),
                    )?)
                }
            }
            Kind::Scw => Box::new(models::job::Job::<SCW>::new(
                context,
                self.long_id,
                self.name,
                self.action.to_service_action(),
                image_source,
                self.schedule,
                self.max_nb_restart,
                Duration::from_secs(self.max_duration_in_sec),
                self.default_port,
                self.command_args,
                self.entrypoint,
                self.force_trigger,
                self.cpu_request_in_milli,
                self.cpu_limit_in_milli,
                self.ram_request_in_mib,
                self.ram_limit_in_mib,
                environment_variables,
                self.advanced_settings,
                ScwAppExtraSettings {},
                |transmitter| context.get_event_details(transmitter),
            )?),
        };

        Ok(service)
    }
}

use crate::build_platform::{Build, GitRepository, Image, SshKey};
use crate::cloud_provider::kubernetes::{Kind as KubernetesKind, Kubernetes};
use crate::cloud_provider::models::CpuArchitecture;
use crate::cloud_provider::service::ServiceType;
use crate::cloud_provider::{CloudProvider, Kind};
use crate::container_registry::{ContainerRegistry, ContainerRegistryInfo};
use crate::engine_task::qovery_api::QoveryApi;
use crate::io_models::application::{to_environment_variable, GitCredentials};
use crate::io_models::container::Registry;
use crate::io_models::context::Context;
use crate::io_models::probe::Probe;
use crate::io_models::variable_utils::{default_environment_vars_with_info, VariableInfo};
use crate::io_models::{
    fetch_git_token, normalize_root_and_dockerfile_path, ssh_keys_from_env_vars, Action, MountedFile,
};
use crate::models;
use crate::models::aws::AwsAppExtraSettings;
use crate::models::aws_ec2::AwsEc2AppExtraSettings;
use crate::models::gcp::GcpAppExtraSettings;
use crate::models::job::{ImageSource, JobError, JobService};
use crate::models::registry_image_source::RegistryImageSource;
use crate::models::scaleway::ScwAppExtraSettings;
use crate::models::types::{AWSEc2, AWS, GCP, SCW};
use crate::utilities::to_short_id;
use base64::engine::general_purpose;
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
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

    // Deployment
    #[serde(alias = "deployment.termination_grace_period_seconds")]
    pub deployment_termination_grace_period_seconds: u32,
    #[serde(alias = "deployment.affinity.node.required")]
    pub deployment_affinity_node_required: BTreeMap<String, String>,

    // Build
    #[serde(alias = "build.timeout_max_sec")]
    pub build_timeout_max_sec: u32,
    #[serde(alias = "build.cpu_max_in_milli")]
    pub build_cpu_max_in_milli: u32,
    #[serde(alias = "build.ram_max_in_gib")]
    pub build_ram_max_in_gib: u32,

    #[serde(alias = "security.service_account_name")]
    pub security_service_account_name: String,
    #[serde(alias = "security.read_only_root_filesystem")]
    pub security_read_only_root_filesystem: bool,
}

impl Default for JobAdvancedSettings {
    fn default() -> Self {
        Self {
            job_delete_ttl_seconds_after_finished: None,
            deployment_termination_grace_period_seconds: 60,
            deployment_affinity_node_required: BTreeMap::new(),
            cronjob_concurrency_policy: "Forbid".to_string(),
            cronjob_failed_jobs_history_limit: 1,
            cronjob_success_jobs_history_limit: 1,
            build_timeout_max_sec: 30 * 60, // 30 minutes
            build_cpu_max_in_milli: 4000,
            build_ram_max_in_gib: 8,
            security_service_account_name: "".to_string(),
            security_read_only_root_filesystem: false,
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
    pub kube_name: String,
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
    #[serde(default = "default_environment_vars_with_info")]
    pub environment_vars_with_infos: BTreeMap<String, VariableInfo>,

    #[serde(default)]
    pub mounted_files: Vec<MountedFile>,
    pub readiness_probe: Option<Probe>,
    pub liveness_probe: Option<Probe>,
    #[serde(default)]
    pub advanced_settings: JobAdvancedSettings,
}

impl Job {
    pub fn to_build(
        &self,
        registry_url: &ContainerRegistryInfo,
        qovery_api: Arc<dyn QoveryApi>,
        architectures: Vec<CpuArchitecture>,
    ) -> Option<Build> {
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
        let ssh_keys: Vec<SshKey> = ssh_keys_from_env_vars(&self.environment_vars_with_infos);

        // Convert our root path to an relative path to be able to append them correctly
        let (root_path, dockerfile_path) = normalize_root_and_dockerfile_path(root_path, dockerfile_path);

        let url = Url::parse(git_url).unwrap_or_else(|_| {
            Url::parse("https://invalid-git-url.com").expect("Error while trying to parse invalid git url")
        });

        let mut disable_build_cache = false;
        let mut build = Build {
            git_repository: GitRepository {
                url,
                get_credentials: if git_credentials.is_none() {
                    None
                } else {
                    let id = self.long_id;
                    Some(Box::new(move || fetch_git_token(&*qovery_api, ServiceType::Job, &id)))
                },
                ssh_keys,
                commit_id: commit_id.clone(),
                dockerfile_path,
                root_path,
                buildpack_language: None,
            },
            image: self.to_image(commit_id.to_string(), registry_url),
            environment_variables: self
                .environment_vars_with_infos
                .iter()
                .filter_map(|(k, variable_infos)| {
                    // Remove special vars
                    let v = String::from_utf8_lossy(
                        &general_purpose::STANDARD
                            .decode(variable_infos.value.as_bytes())
                            .unwrap_or_default(),
                    )
                    .into_owned();
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
        Some(build)
    }

    fn to_image(&self, commit_id: String, cr_info: &ContainerRegistryInfo) -> Image {
        Image {
            service_id: to_short_id(&self.long_id),
            service_long_id: self.long_id,
            service_name: self.name.clone(),
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
        cluster: &dyn Kubernetes,
    ) -> Result<Box<dyn JobService>, JobError> {
        let image_source = match self.source {
            JobSource::Docker { .. } => {
                let build = match self.to_build(
                    default_container_registry.registry_info(),
                    context.qovery_api.clone(),
                    cluster.cpu_architectures(),
                ) {
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
                    source: Box::new(RegistryImageSource {
                        registry,
                        image,
                        tag,
                        registry_mirroring_mode: cluster.advanced_settings().registry_mirroring_mode.clone(),
                    }),
                }
            }
        };

        let environment_variables = to_environment_variable(self.environment_vars_with_infos);

        let service: Box<dyn JobService> = match cloud_provider.kind() {
            Kind::Aws => {
                if cloud_provider.kubernetes_kind() == KubernetesKind::Eks {
                    Box::new(models::job::Job::<AWS>::new(
                        context,
                        self.long_id,
                        self.name,
                        self.kube_name,
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
                        self.mounted_files
                            .iter()
                            .map(|e| e.to_domain())
                            .collect::<BTreeSet<_>>(),
                        self.advanced_settings,
                        self.readiness_probe.map(|p| p.to_domain()),
                        self.liveness_probe.map(|p| p.to_domain()),
                        AwsAppExtraSettings {},
                        |transmitter| context.get_event_details(transmitter),
                    )?)
                } else {
                    Box::new(models::job::Job::<AWSEc2>::new(
                        context,
                        self.long_id,
                        self.name,
                        self.kube_name,
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
                        self.mounted_files
                            .iter()
                            .map(|e| e.to_domain())
                            .collect::<BTreeSet<_>>(),
                        self.advanced_settings,
                        self.readiness_probe.map(|p| p.to_domain()),
                        self.liveness_probe.map(|p| p.to_domain()),
                        AwsEc2AppExtraSettings {},
                        |transmitter| context.get_event_details(transmitter),
                    )?)
                }
            }
            Kind::Scw => Box::new(models::job::Job::<SCW>::new(
                context,
                self.long_id,
                self.name,
                self.kube_name,
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
                self.mounted_files
                    .iter()
                    .map(|e| e.to_domain())
                    .collect::<BTreeSet<_>>(),
                self.advanced_settings,
                self.readiness_probe.map(|p| p.to_domain()),
                self.liveness_probe.map(|p| p.to_domain()),
                ScwAppExtraSettings {},
                |transmitter| context.get_event_details(transmitter),
            )?),
            Kind::Gcp => Box::new(models::job::Job::<GCP>::new(
                context,
                self.long_id,
                self.name,
                self.kube_name,
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
                self.mounted_files
                    .iter()
                    .map(|e| e.to_domain())
                    .collect::<BTreeSet<_>>(),
                self.advanced_settings,
                self.readiness_probe.map(|p| p.to_domain()),
                self.liveness_probe.map(|p| p.to_domain()),
                GcpAppExtraSettings {},
                |transmitter| context.get_event_details(transmitter),
            )?),
        };

        Ok(service)
    }
}

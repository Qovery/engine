use crate::cloud_provider::kubernetes::Kind as KubernetesKind;
use crate::cloud_provider::{CloudProvider, Kind};
use crate::container_registry::ContainerRegistry;
use crate::io_models::application::{to_environment_variable, AdvancedSettingsProbeType, GitCredentials};
use crate::io_models::container::Registry;
use crate::io_models::context::Context;
use crate::io_models::Action;
use crate::models;
use crate::models::aws::AwsAppExtraSettings;
use crate::models::aws_ec2::AwsEc2AppExtraSettings;
use crate::models::job::{JobError, JobService};
use crate::models::scaleway::ScwAppExtraSettings;
use crate::models::types::{AWSEc2, AWS, SCW};
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
            readiness_probe_type: AdvancedSettingsProbeType::Tcp,
            readiness_probe_http_get_path: "/".to_string(),
            readiness_probe_initial_delay_seconds: 30,
            readiness_probe_period_seconds: 10,
            readiness_probe_timeout_seconds: 5,
            readiness_probe_success_threshold: 1,
            readiness_probe_failure_threshold: 3,
            liveness_probe_type: AdvancedSettingsProbeType::Tcp,
            liveness_probe_http_get_path: "/".to_string(),
            liveness_probe_initial_delay_seconds: 30,
            liveness_probe_period_seconds: 10,
            liveness_probe_timeout_seconds: 5,
            liveness_probe_success_threshold: 1,
            liveness_probe_failure_threshold: 3,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub enum JobSchedule {
    OnStart,
    OnPause,
    OnDelete,
    Cron(String),
}
impl JobSchedule {
    pub fn is_cronjob(&self) -> bool {
        matches!(self, JobSchedule::Cron(_))
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
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
    pub max_nb_restart: u32,           // .spec.backoffLimit
    pub max_duration_in_sec: Duration, // .spec.activeDeadlineSeconds
    pub default_port: Option<u16>,     // for probes
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
    pub fn to_job_domain(
        self,
        context: &Context,
        cloud_provider: &dyn CloudProvider,
        default_container_registry: &dyn ContainerRegistry,
    ) -> Result<Box<dyn JobService>, JobError> {
        let environment_variables = to_environment_variable(self.environment_vars);

        // FIXME: Remove this after we support launching job with something else than an already built container
        let mut registry_ = Registry::DockerHub {
            long_id: Default::default(),
            url: Url::parse("https://default.com").unwrap(),
            credentials: None,
        };
        let mut image_ = "".to_string();
        let mut tag_ = "".to_string();

        match self.source {
            JobSource::Docker { .. } => {}
            JobSource::Image {
                mut registry,
                image,
                tag,
            } => {
                // Default registry is a bit special as the core does not knows its url/credentials as it is retrieved by us with some tags
                if registry.id() == default_container_registry.long_id() {
                    registry.set_url(default_container_registry.registry_info().endpoint.clone());
                }
                registry_ = registry;
                image_ = image;
                tag_ = tag;
            }
        }

        let service: Box<dyn JobService> = match cloud_provider.kind() {
            Kind::Aws => {
                if cloud_provider.kubernetes_kind() == KubernetesKind::Eks {
                    Box::new(models::job::Job::<AWS>::new(
                        context,
                        self.long_id,
                        self.name,
                        self.action.to_service_action(),
                        registry_,
                        image_,
                        tag_,
                        self.schedule,
                        self.max_nb_restart,
                        self.max_duration_in_sec,
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
                        registry_,
                        image_,
                        tag_,
                        self.schedule,
                        self.max_nb_restart,
                        self.max_duration_in_sec,
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
                registry_,
                image_,
                tag_,
                self.schedule,
                self.max_nb_restart,
                self.max_duration_in_sec,
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
            Kind::Do => {
                unimplemented!("DO is not implemented")
            }
        };

        Ok(service)
    }
}

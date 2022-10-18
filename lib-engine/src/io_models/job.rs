
use crate::cloud_provider::{CloudProvider};
use crate::container_registry::ContainerRegistry;
use crate::io_models::application::{AdvancedSettingsProbeType, GitCredentials};
use crate::io_models::container::Registry;
use crate::io_models::context::Context;
use crate::io_models::Action;
use crate::logger::Logger;
use crate::models::container::{ContainerError, ContainerService};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
#[serde(default)]
pub struct JobAdvancedSettings {
    // Job specific
    #[serde(alias = "job.delete_ttl_seconds_after_finished")]
    pub delete_ttl_seconds_after_finished: Duration,

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
            delete_ttl_seconds_after_finished: Duration::from_secs(0),
            readiness_probe_type: AdvancedSettingsProbeType::Tcp,
            readiness_probe_http_get_path: "/".to_string(),
            readiness_probe_initial_delay_seconds: 30,
            readiness_probe_period_seconds: 10,
            readiness_probe_timeout_seconds: 1,
            readiness_probe_success_threshold: 1,
            readiness_probe_failure_threshold: 9,
            liveness_probe_type: AdvancedSettingsProbeType::Tcp,
            liveness_probe_http_get_path: "/".to_string(),
            liveness_probe_initial_delay_seconds: 30,
            liveness_probe_period_seconds: 10,
            liveness_probe_timeout_seconds: 5,
            liveness_probe_success_threshold: 1,
            liveness_probe_failure_threshold: 9,
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
    pub nb_restart_limit: u32,         // .spec.backoffLimit
    pub max_duration_in_sec: Duration, // .spec.activeDeadlineSeconds
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
        _context: &Context,
        _cloud_provider: &dyn CloudProvider,
        _default_container_registry: &dyn ContainerRegistry,
        _logger: Box<dyn Logger>,
    ) -> Result<Box<dyn ContainerService>, ContainerError> {
        todo!()
    }
}

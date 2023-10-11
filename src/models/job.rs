use crate::build_platform::Build;
use crate::cloud_provider::models::{EnvironmentVariable, MountedFile};
use crate::cloud_provider::service::{Action, Service, ServiceType};
use crate::cloud_provider::DeploymentTarget;
use crate::deployment_action::DeploymentAction;
use crate::events::{EventDetails, Stage, Transmitter};
use crate::io_models::context::Context;
use crate::io_models::job::{JobAdvancedSettings, JobSchedule};
use crate::models;
use crate::models::container::RegistryTeraContext;
use crate::models::probe::Probe;
use crate::models::registry_image_source::RegistryImageSource;
use crate::models::types::{CloudProvider, ToTeraContext};
use crate::utilities::to_short_id;
use serde::Serialize;
use std::collections::BTreeSet;
use std::marker::PhantomData;
use std::time::Duration;
use uuid::Uuid;

#[derive(thiserror::Error, Debug)]
pub enum JobError {
    #[error("Job invalid configuration: {0}")]
    InvalidConfig(String),
}

pub struct Job<T: CloudProvider> {
    _marker: PhantomData<T>,
    pub(super) mk_event_details: Box<dyn Fn(Stage) -> EventDetails + Send + Sync>,
    pub(super) id: String,
    pub(super) long_id: Uuid,
    pub(super) name: String,
    pub(super) kube_name: String,
    pub(super) action: Action,
    pub image_source: ImageSource,
    pub(super) schedule: JobSchedule,
    pub(super) max_nb_restart: u32,
    pub(super) max_duration: Duration,
    pub(super) default_port: Option<u16>, // for probes
    pub(super) command_args: Vec<String>,
    pub(super) entrypoint: Option<String>,
    pub(super) force_trigger: bool,
    pub(super) cpu_request_in_milli: u32,
    pub(super) cpu_limit_in_milli: u32,
    pub(super) ram_request_in_mib: u32,
    pub(super) ram_limit_in_mib: u32,
    pub(super) environment_variables: Vec<EnvironmentVariable>,
    pub(super) mounted_files: BTreeSet<MountedFile>,
    pub(super) advanced_settings: JobAdvancedSettings,
    pub(super) _extra_settings: T::AppExtraSettings,
    pub(super) workspace_directory: String,
    pub(super) lib_root_directory: String,
    pub(super) readiness_probe: Option<Probe>,
    pub(super) liveness_probe: Option<Probe>,
}

// Here we define the common behavior among all providers
impl<T: CloudProvider> Job<T> {
    pub fn new(
        context: &Context,
        long_id: Uuid,
        name: String,
        kube_name: String,
        action: Action,
        image_source: ImageSource,
        schedule: JobSchedule,
        max_nb_restart: u32,
        max_duration_in_sec: Duration,
        default_port: Option<u16>, // for probes
        command_args: Vec<String>,
        entrypoint: Option<String>,
        force_trigger: bool,
        cpu_request_in_milli: u32,
        cpu_limit_in_milli: u32,
        ram_request_in_mib: u32,
        ram_limit_in_mib: u32,
        environment_variables: Vec<EnvironmentVariable>,
        mounted_files: BTreeSet<MountedFile>,
        advanced_settings: JobAdvancedSettings,
        readiness_probe: Option<Probe>,
        liveness_probe: Option<Probe>,
        extra_settings: T::AppExtraSettings,
        mk_event_details: impl Fn(Transmitter) -> EventDetails,
    ) -> Result<Self, JobError> {
        if cpu_request_in_milli > cpu_limit_in_milli {
            return Err(JobError::InvalidConfig(
                "cpu_request_in_mili must be less or equal to cpu_limit_in_mili".to_string(),
            ));
        }

        if cpu_request_in_milli == 0 {
            return Err(JobError::InvalidConfig(
                "cpu_request_in_mili must be greater than 0".to_string(),
            ));
        }

        if ram_request_in_mib > ram_limit_in_mib {
            return Err(JobError::InvalidConfig(
                "ram_request_in_mib must be less or equal to ram_limit_in_mib".to_string(),
            ));
        }

        if ram_request_in_mib == 0 {
            return Err(JobError::InvalidConfig("ram_request_in_mib must be greater than 0".to_string()));
        }

        let workspace_directory = crate::fs::workspace_directory(
            context.workspace_root_dir(),
            context.execution_id(),
            format!("jobs/{long_id}"),
        )
        .map_err(|err| JobError::InvalidConfig(format!("Can't create workspace directory: {err}")))?;

        let event_details = mk_event_details(Transmitter::Job(long_id, name.to_string()));
        let mk_event_details = move |stage: Stage| EventDetails::clone_changing_stage(event_details.clone(), stage);
        Ok(Self {
            _marker: PhantomData,
            mk_event_details: Box::new(mk_event_details),
            id: to_short_id(&long_id),
            long_id,
            action,
            image_source,
            schedule,
            max_nb_restart,
            max_duration: max_duration_in_sec,
            name,
            kube_name,
            command_args,
            entrypoint,
            force_trigger,
            cpu_request_in_milli,
            cpu_limit_in_milli,
            ram_request_in_mib,
            ram_limit_in_mib,
            environment_variables,
            mounted_files,
            advanced_settings,
            _extra_settings: extra_settings,
            workspace_directory,
            readiness_probe,
            liveness_probe,
            lib_root_directory: context.lib_root_dir().to_string(),
            default_port,
        })
    }

    pub fn helm_selector(&self) -> Option<String> {
        Some(self.kube_label_selector())
    }

    pub fn helm_release_name(&self) -> String {
        format!("job-{}", self.long_id)
    }

    pub fn helm_chart_dir(&self) -> String {
        format!("{}/common/charts/q-job", self.lib_root_directory)
    }

    pub fn schedule(&self) -> &JobSchedule {
        &self.schedule
    }

    pub fn should_force_trigger(&self) -> bool {
        self.force_trigger
    }

    pub fn max_nb_restart(&self) -> u32 {
        self.max_nb_restart
    }

    pub(super) fn default_tera_context(&self, target: &DeploymentTarget) -> JobTeraContext {
        let environment = &target.environment;
        let kubernetes = &target.kubernetes;
        let registry_info = target.container_registry.registry_info();
        let (image_full, image_tag) = match &self.image_source {
            ImageSource::Registry { source } => {
                let image_tag = source.tag_for_mirror(&self.long_id);
                (
                    format!(
                        "{}/{}:{}",
                        registry_info.endpoint.host_str().unwrap_or_default(),
                        (registry_info.get_image_name)(&models::container::get_mirror_repository_name(
                            self.long_id(),
                            target.kubernetes.long_id(),
                            &target.kubernetes.advanced_settings().registry_mirroring_mode,
                        )),
                        image_tag
                    ),
                    image_tag,
                )
            }
            ImageSource::Build { source } => (source.image.full_image_name_with_tag(), source.image.tag.clone()),
        };

        let ctx = JobTeraContext {
            organization_long_id: environment.organization_long_id,
            project_long_id: environment.project_long_id,
            environment_short_id: to_short_id(&environment.long_id),
            environment_long_id: environment.long_id,
            cluster: ClusterTeraContext {
                long_id: *kubernetes.long_id(),
                name: kubernetes.name().to_string(),
                region: kubernetes.region().to_string(),
                zone: kubernetes.zone().to_string(),
            },
            namespace: environment.namespace().to_string(),
            service: ServiceTeraContext {
                short_id: to_short_id(&self.long_id),
                long_id: self.long_id,
                name: self.kube_name().to_string(),
                version: self.service_version(),
                user_unsafe_name: self.name.clone(),
                image_full,
                image_tag,
                command_args: self.command_args.clone(),
                entrypoint: self.entrypoint.clone(),
                cpu_request_in_milli: format!("{}m", self.cpu_request_in_milli),
                cpu_limit_in_milli: format!("{}m", self.cpu_limit_in_milli),
                ram_request_in_mib: format!("{}Mi", self.ram_request_in_mib),
                ram_limit_in_mib: format!("{}Mi", self.ram_limit_in_mib),
                default_port: self.default_port,
                max_nb_restart: self.max_nb_restart,
                max_duration_in_sec: self.max_duration.as_secs(),
                cronjob_schedule: match &self.schedule {
                    JobSchedule::OnStart {} | JobSchedule::OnPause {} | JobSchedule::OnDelete {} => None,
                    JobSchedule::Cron { schedule } => Some(schedule.to_string()),
                },
                readiness_probe: self.readiness_probe.clone(),
                liveness_probe: self.liveness_probe.clone(),
                advanced_settings: self.advanced_settings.clone(),
            },
            registry: registry_info
                .registry_docker_json_config
                .as_ref()
                .map(|docker_json| RegistryTeraContext {
                    secret_name: format!("{}-registry", self.kube_name()),
                    docker_json_config: Some(docker_json.to_string()),
                }),
            environment_variables: self.environment_variables.clone(),
            mounted_files: self.mounted_files.clone().into_iter().collect::<Vec<_>>(),
            resource_expiration_in_seconds: Some(kubernetes.advanced_settings().pleco_resources_ttl),
        };

        ctx
    }

    pub fn service_type(&self) -> ServiceType {
        ServiceType::Job
    }

    pub fn id(&self) -> &str {
        self.id.as_str()
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub fn action(&self) -> &Action {
        &self.action
    }

    pub fn image_with_tag(&self) -> String {
        match &self.image_source {
            ImageSource::Registry { source: registry } => format!("{}:{}", registry.image, registry.tag),
            ImageSource::Build { source: build } => build.image.name_without_repository().to_string(),
        }
    }

    fn service_version(&self) -> String {
        match &self.image_source {
            ImageSource::Registry { source: registry } => {
                format!("{}:{}", registry.image, registry.tag)
            }
            ImageSource::Build { source: build } => build.git_repository.commit_id.clone(),
        }
    }

    pub fn is_cron_job(&self) -> bool {
        matches!(self.schedule, JobSchedule::Cron { .. })
    }

    pub fn kube_label_selector(&self) -> String {
        format!("qovery.com/service-id={}", self.long_id)
    }

    pub fn workspace_directory(&self) -> &str {
        &self.workspace_directory
    }
}

impl<T: CloudProvider> Service for Job<T> {
    fn service_type(&self) -> ServiceType {
        self.service_type()
    }

    fn id(&self) -> &str {
        self.id()
    }

    fn long_id(&self) -> &Uuid {
        &self.long_id
    }

    fn name(&self) -> &str {
        self.name()
    }

    fn version(&self) -> String {
        self.service_version()
    }

    fn kube_name(&self) -> &str {
        &self.kube_name
    }

    fn kube_label_selector(&self) -> String {
        self.kube_label_selector()
    }

    fn get_event_details(&self, stage: Stage) -> EventDetails {
        (self.mk_event_details)(stage)
    }

    fn action(&self) -> &Action {
        self.action()
    }

    fn as_service(&self) -> &dyn Service {
        self
    }

    fn as_service_mut(&mut self) -> &mut dyn Service {
        self
    }

    fn build(&self) -> Option<&Build> {
        match &self.image_source {
            ImageSource::Registry { .. } => None,
            ImageSource::Build { source: build } if self.force_trigger => Some(build),
            ImageSource::Build { source: build } => match &self.schedule {
                JobSchedule::OnStart { .. } if self.action == Action::Create => Some(build),
                JobSchedule::OnPause { .. } if self.action == Action::Pause => Some(build),
                JobSchedule::OnDelete { .. } if self.action == Action::Delete => Some(build),
                JobSchedule::Cron { .. } if self.action == Action::Create => Some(build),
                _ => None,
            },
        }
    }

    fn build_mut(&mut self) -> Option<&mut Build> {
        match &mut self.image_source {
            ImageSource::Registry { .. } => None,
            ImageSource::Build { source: build } => {
                if self.force_trigger {
                    return Some(build);
                }

                match &self.schedule {
                    JobSchedule::OnStart { .. } if self.action == Action::Create => Some(build),
                    JobSchedule::OnPause { .. } if self.action == Action::Pause => Some(build),
                    JobSchedule::OnDelete { .. } if self.action == Action::Delete => Some(build),
                    JobSchedule::Cron { .. } if self.action == Action::Create => Some(build),
                    _ => None,
                }
            }
        }
    }
}

pub trait JobService: Service + DeploymentAction + ToTeraContext + Send {
    fn advanced_settings(&self) -> &JobAdvancedSettings;
    fn image_full(&self) -> String;
    fn startup_timeout(&self) -> Duration;
    fn as_deployment_action(&self) -> &dyn DeploymentAction;
    fn job_schedule(&self) -> &JobSchedule;
    fn max_duration(&self) -> &Duration;
    fn max_restarts(&self) -> u32;
    fn is_force_trigger(&self) -> bool;
}

impl<T: CloudProvider> JobService for Job<T>
where
    Job<T>: Service + ToTeraContext + DeploymentAction,
{
    fn advanced_settings(&self) -> &JobAdvancedSettings {
        &self.advanced_settings
    }

    fn image_full(&self) -> String {
        match &self.image_source {
            ImageSource::Registry { source: registry } => {
                format!(
                    "{}{}:{}",
                    registry.registry.url().to_string().trim_start_matches("https://"),
                    registry.image,
                    registry.tag
                )
            }
            ImageSource::Build { source: build } => build.image.full_image_name_with_tag(),
        }
    }

    fn startup_timeout(&self) -> Duration {
        let readiness_probe_timeout = if let Some(p) = &self.readiness_probe {
            p.initial_delay_seconds + ((p.timeout_seconds + p.period_seconds) * p.failure_threshold)
        } else {
            60 * 5
        };

        let liveness_probe_timeout = if let Some(p) = &self.liveness_probe {
            p.initial_delay_seconds + ((p.timeout_seconds + p.period_seconds) * p.failure_threshold)
        } else {
            60 * 5
        };

        let probe_timeout = std::cmp::max(readiness_probe_timeout, liveness_probe_timeout);
        let startup_timeout = std::cmp::max(probe_timeout /* * 10 rolling restart percent */, 60 * 10);
        Duration::from_secs(startup_timeout as u64)
    }

    fn as_deployment_action(&self) -> &dyn DeploymentAction {
        self
    }

    fn job_schedule(&self) -> &JobSchedule {
        &self.schedule
    }

    fn max_duration(&self) -> &Duration {
        &self.max_duration
    }

    fn max_restarts(&self) -> u32 {
        self.max_nb_restart
    }

    fn is_force_trigger(&self) -> bool {
        self.force_trigger
    }
}

pub enum ImageSource {
    Registry { source: Box<RegistryImageSource> },
    Build { source: Box<Build> },
}

#[derive(Serialize, Debug, Clone)]
pub(super) struct ClusterTeraContext {
    pub(super) long_id: Uuid,
    pub(super) name: String,
    pub(super) region: String,
    pub(super) zone: String,
}

#[derive(Serialize, Debug, Clone)]
pub(super) struct ServiceTeraContext {
    pub(super) short_id: String,
    pub(super) long_id: Uuid,
    pub(super) name: String,
    pub(super) version: String,
    pub(super) user_unsafe_name: String,
    pub(super) image_full: String,
    pub(super) image_tag: String,
    pub(super) command_args: Vec<String>,
    pub(super) entrypoint: Option<String>,
    pub(super) cpu_request_in_milli: String,
    pub(super) cpu_limit_in_milli: String,
    pub(super) ram_request_in_mib: String,
    pub(super) ram_limit_in_mib: String,
    pub(super) default_port: Option<u16>,
    pub(super) max_nb_restart: u32,
    pub(super) max_duration_in_sec: u64,
    pub(super) cronjob_schedule: Option<String>,
    pub(super) readiness_probe: Option<Probe>,
    pub(super) liveness_probe: Option<Probe>,
    pub(super) advanced_settings: JobAdvancedSettings,
}

#[derive(Serialize, Debug, Clone)]
pub(super) struct JobTeraContext {
    pub(super) organization_long_id: Uuid,
    pub(super) project_long_id: Uuid,
    pub(super) environment_short_id: String,
    pub(super) environment_long_id: Uuid,
    pub(super) cluster: ClusterTeraContext,
    pub(super) namespace: String,
    pub(super) service: ServiceTeraContext,
    pub(super) registry: Option<RegistryTeraContext>,
    pub(super) environment_variables: Vec<EnvironmentVariable>,
    pub(super) mounted_files: Vec<MountedFile>,
    pub(super) resource_expiration_in_seconds: Option<i32>,
}

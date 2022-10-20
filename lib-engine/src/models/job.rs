use crate::cloud_provider::models::EnvironmentVariable;
use crate::cloud_provider::service::{Action, Service, ServiceType};
use crate::cloud_provider::DeploymentTarget;
use crate::deployment_action::DeploymentAction;
use crate::errors::EngineError;
use crate::events::{EventDetails, Stage, Transmitter};
use crate::io_models::container::Registry;
use crate::io_models::context::Context;
use crate::io_models::job::{JobAdvancedSettings, JobSchedule};
use crate::models;
use crate::models::container::RegistryTeraContext;
use crate::models::types::{CloudProvider, ToTeraContext};
use crate::string::cut;
use crate::utilities::to_short_id;
use serde::Serialize;
use std::marker::PhantomData;
use std::time::Duration;
use tera::Context as TeraContext;
use uuid::Uuid;

#[derive(thiserror::Error, Debug)]
pub enum JobError {
    #[error("Job invalid configuration: {0}")]
    InvalidConfig(String),
}

pub struct Job<T: CloudProvider> {
    _marker: PhantomData<T>,
    pub(super) mk_event_details: Box<dyn Fn(Stage) -> EventDetails>,
    pub(super) id: String,
    pub(super) long_id: Uuid,
    pub(super) name: String,
    pub(super) action: Action,
    pub registry: Registry,
    pub image: String,
    pub tag: String,
    pub(super) schedule: JobSchedule,
    pub(super) max_nb_restart: u32,
    pub(super) max_duration_in_sec: Duration,
    pub(super) default_port: Option<u16>, // for probes
    pub(super) command_args: Vec<String>,
    pub(super) entrypoint: Option<String>,
    pub(super) force_trigger: bool,
    pub(super) cpu_request_in_milli: u32,
    pub(super) cpu_limit_in_milli: u32,
    pub(super) ram_request_in_mib: u32,
    pub(super) ram_limit_in_mib: u32,
    pub(super) environment_variables: Vec<EnvironmentVariable>,
    pub(super) advanced_settings: JobAdvancedSettings,
    pub(super) _extra_settings: T::AppExtraSettings,
    pub(super) workspace_directory: String,
    pub(super) lib_root_directory: String,
}

// Here we define the common behavior among all providers
impl<T: CloudProvider> Job<T> {
    pub fn new(
        context: &Context,
        long_id: Uuid,
        name: String,
        action: Action,
        registry: Registry,
        image: String,
        tag: String,
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
        advanced_settings: JobAdvancedSettings,
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
            format!("jobs/{}", long_id),
        )
        .map_err(|err| JobError::InvalidConfig(format!("Can't create workspace directory: {}", err)))?;

        let event_details = mk_event_details(Transmitter::Job(long_id, name.to_string()));
        let mk_event_details = move |stage: Stage| EventDetails::clone_changing_stage(event_details.clone(), stage);
        Ok(Self {
            _marker: PhantomData,
            mk_event_details: Box::new(mk_event_details),
            id: to_short_id(&long_id),
            long_id,
            action,
            registry,
            image,
            tag,
            schedule,
            max_nb_restart,
            max_duration_in_sec,
            name,
            command_args,
            entrypoint,
            force_trigger,
            cpu_request_in_milli,
            cpu_limit_in_milli,
            ram_request_in_mib,
            ram_limit_in_mib,
            environment_variables,
            advanced_settings,
            _extra_settings: extra_settings,
            workspace_directory,
            lib_root_directory: context.lib_root_dir().to_string(),
            default_port,
        })
    }

    pub fn helm_selector(&self) -> Option<String> {
        Some(self.selector())
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

    fn kube_service_name(&self) -> String {
        format!("job-{}", to_short_id(&self.long_id))
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub fn should_force_trigger(&self) -> bool {
        self.force_trigger
    }

    pub(super) fn default_tera_context(&self, target: &DeploymentTarget) -> JobTeraContext {
        let environment = &target.environment;
        let kubernetes = &target.kubernetes;
        let registry_info = target.container_registry.registry_info();

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
                name: self.kube_service_name(),
                user_unsafe_name: self.name.clone(),
                // FIXME: We mirror images to cluster private registry
                image_full: format!(
                    "{}/{}:{}",
                    registry_info.endpoint.host_str().unwrap_or_default(),
                    (registry_info.get_image_name)(models::container::QOVERY_MIRROR_REPOSITORY_NAME),
                    self.tag_for_mirror()
                ),
                image_tag: self.tag_for_mirror(),
                command_args: self.command_args.clone(),
                entrypoint: self.entrypoint.clone(),
                cpu_request_in_milli: format!("{}m", self.cpu_request_in_milli),
                cpu_limit_in_milli: format!("{}m", self.cpu_limit_in_milli),
                ram_request_in_mib: format!("{}Mi", self.ram_request_in_mib),
                ram_limit_in_mib: format!("{}Mi", self.ram_limit_in_mib),
                default_port: self.default_port,
                max_nb_restart: self.max_nb_restart,
                max_duration_in_sec: self.max_duration_in_sec.as_secs(),
                cronjob_schedule: match &self.schedule {
                    JobSchedule::OnStart | JobSchedule::OnPause | JobSchedule::OnDelete => None,
                    JobSchedule::Cron(schedule) => Some(schedule.to_string()),
                },
                advanced_settings: self.advanced_settings.clone(),
            },
            registry: registry_info
                .registry_docker_json_config
                .as_ref()
                .map(|docker_json| RegistryTeraContext {
                    secret_name: format!("{}-registry", self.kube_service_name()),
                    docker_json_config: docker_json.to_string(),
                }),
            environment_variables: self.environment_variables.clone(),
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
        format!("{}:{}", self.image, self.tag)
    }

    pub fn is_cron_job(&self) -> bool {
        matches!(self.schedule, JobSchedule::Cron(_))
    }

    pub fn tag_for_mirror(&self) -> String {
        // A tag name must be valid ASCII and may contain lowercase and uppercase letters, digits, underscores, periods and dashes.
        // A tag name may not start with a period or a dash and may contain a maximum of 128 characters.
        cut(format!("{}.{}.{}", self.image.replace('/', "."), self.tag, self.long_id), 128)
    }

    pub fn selector(&self) -> String {
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

    fn sanitized_name(&self) -> String {
        panic!("don't use that, it is deprecated");
    }

    fn get_event_details(&self, stage: Stage) -> EventDetails {
        (self.mk_event_details)(stage)
    }

    fn action(&self) -> &Action {
        self.action()
    }

    fn selector(&self) -> Option<String> {
        Some(self.selector())
    }

    fn as_service(&self) -> &dyn Service {
        self
    }
}

impl<T: CloudProvider> ToTeraContext for Job<T> {
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        Ok(TeraContext::from_serialize(self.default_tera_context(target)).unwrap_or_default())
    }
}

pub trait JobService: Service + DeploymentAction + ToTeraContext {
    fn advanced_settings(&self) -> &JobAdvancedSettings;
    fn image_full(&self) -> String;
    fn kube_service_name(&self) -> String;
    fn startup_timeout(&self) -> Duration {
        let settings = self.advanced_settings();
        let readiness_probe_timeout = settings.readiness_probe_initial_delay_seconds
            + ((settings.readiness_probe_timeout_seconds + settings.readiness_probe_period_seconds)
                * settings.readiness_probe_failure_threshold);
        let liveness_probe_timeout = settings.liveness_probe_initial_delay_seconds
            + ((settings.liveness_probe_timeout_seconds + settings.liveness_probe_period_seconds)
                * settings.liveness_probe_failure_threshold);
        let probe_timeout = std::cmp::max(readiness_probe_timeout, liveness_probe_timeout);
        let startup_timeout = std::cmp::max(probe_timeout /* * 10 rolling restart percent */, 60 * 10);
        Duration::from_secs(startup_timeout as u64)
    }

    fn as_deployment_action(&self) -> &dyn DeploymentAction;
    fn cronjob_schedule(&self) -> Option<&str>;
}

impl<T: CloudProvider> JobService for Job<T>
where
    Job<T>: Service + ToTeraContext + DeploymentAction,
{
    fn advanced_settings(&self) -> &JobAdvancedSettings {
        &self.advanced_settings
    }

    fn image_full(&self) -> String {
        format!(
            "{}{}:{}",
            self.registry.url().to_string().trim_start_matches("https://"),
            self.image,
            self.tag
        )
    }

    fn kube_service_name(&self) -> String {
        self.kube_service_name()
    }

    fn as_deployment_action(&self) -> &dyn DeploymentAction {
        self
    }

    fn cronjob_schedule(&self) -> Option<&str> {
        match &self.schedule {
            JobSchedule::OnStart => None,
            JobSchedule::OnPause => None,
            JobSchedule::OnDelete => None,
            JobSchedule::Cron(schedule) => Some(schedule),
        }
    }
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
    pub(super) resource_expiration_in_seconds: Option<i32>,
}

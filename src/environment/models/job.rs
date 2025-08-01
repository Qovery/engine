use crate::environment::action::DeploymentAction;
use crate::environment::models::annotations_group::AnnotationsGroupTeraContext;
use crate::environment::models::container::{ClusterTeraContext, RegistryTeraContext};
use crate::environment::models::labels_group::LabelsGroupTeraContext;
use crate::environment::models::probe::Probe;
use crate::environment::models::registry_image_source::RegistryImageSource;
use crate::environment::models::types::{CloudProvider, ToTeraContext};
use crate::environment::models::utils;
use crate::events::{EventDetails, Stage, Transmitter};
use crate::infrastructure::models::build_platform::Build;
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::infrastructure::models::cloud_provider::service::{Action, Service, ServiceType};
use crate::infrastructure::models::container_registry::DockerRegistryInfo;
use crate::io_models::annotations_group::AnnotationsGroup;
use crate::io_models::context::Context;
use crate::io_models::job::{JobAdvancedSettings, JobSchedule, LifecycleType};
use crate::io_models::labels_group::LabelsGroup;
use crate::io_models::models::{
    EnvironmentVariable, KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit, MountedFile,
};
use crate::utilities::to_short_id;
use serde::Serialize;
use std::borrow::Cow;
use std::collections::BTreeSet;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;

#[derive(thiserror::Error, Debug)]
pub enum JobError {
    #[error("Job invalid configuration: {0}")]
    InvalidConfig(String),
}

pub struct Job<T: CloudProvider> {
    _marker: PhantomData<T>,
    pub(crate) mk_event_details: Box<dyn Fn(Stage) -> EventDetails + Send + Sync>,
    pub(crate) id: String,
    pub(crate) long_id: Uuid,
    pub(crate) deployment_id: String,
    pub(crate) name: String,
    pub(crate) kube_name: String,
    pub(crate) action: Action,
    pub image_source: ImageSource,
    pub(crate) schedule: JobSchedule,
    pub(crate) max_nb_restart: u32,
    pub(crate) max_duration: Duration,
    pub(crate) default_port: Option<u16>,
    // for probes
    pub(crate) command_args: Vec<String>,
    pub(crate) entrypoint: Option<String>,
    pub(crate) force_trigger: bool,
    pub(crate) cpu_request_in_milli: KubernetesCpuResourceUnit,
    pub(crate) cpu_limit_in_milli: KubernetesCpuResourceUnit,
    pub(crate) ram_request_in_mib: KubernetesMemoryResourceUnit,
    pub(crate) ram_limit_in_mib: KubernetesMemoryResourceUnit,
    pub(crate) environment_variables: Vec<EnvironmentVariable>,
    pub(crate) mounted_files: BTreeSet<MountedFile>,
    pub(crate) advanced_settings: JobAdvancedSettings,
    pub(crate) _extra_settings: T::AppExtraSettings,
    pub(crate) workspace_directory: PathBuf,
    pub(crate) lib_root_directory: String,
    pub(crate) readiness_probe: Option<Probe>,
    pub(crate) liveness_probe: Option<Probe>,
    pub(crate) annotations_group: AnnotationsGroupTeraContext,
    pub(crate) labels_group: LabelsGroupTeraContext,
    pub(crate) should_delete_shared_registry: bool,
    pub(crate) output_variable_validation_pattern: String,
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
        cpu_request_in_milli: KubernetesCpuResourceUnit,
        cpu_limit_in_milli: KubernetesCpuResourceUnit,
        ram_request_in_mib: KubernetesMemoryResourceUnit,
        ram_limit_in_mib: KubernetesMemoryResourceUnit,
        environment_variables: Vec<EnvironmentVariable>,
        mounted_files: BTreeSet<MountedFile>,
        advanced_settings: JobAdvancedSettings,
        readiness_probe: Option<Probe>,
        liveness_probe: Option<Probe>,
        extra_settings: T::AppExtraSettings,
        mk_event_details: impl Fn(Transmitter) -> EventDetails,
        annotations_groups: Vec<AnnotationsGroup>,
        labels_groups: Vec<LabelsGroup>,
        should_delete_shared_registry: bool,
        output_variable_validation_pattern: String,
    ) -> Result<Self, JobError> {
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
            deployment_id: context
                .execution_id()
                .rsplit_once('-')
                .map(|s| s.0.to_string())
                .unwrap_or_default(),
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
            annotations_group: AnnotationsGroupTeraContext::new(annotations_groups),
            labels_group: LabelsGroupTeraContext::new(labels_groups),
            should_delete_shared_registry,
            output_variable_validation_pattern,
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

    pub(crate) fn default_tera_context(&self, target: &DeploymentTarget) -> JobTeraContext {
        let environment = target.environment;
        let kubernetes = target.kubernetes;
        let deployment_affinity_node_required = utils::add_arch_to_deployment_affinity_node(
            &self.advanced_settings.deployment_affinity_node_required,
            &target.kubernetes.cpu_architectures(),
        );
        let mut advanced_settings = self.advanced_settings.clone();
        advanced_settings.deployment_affinity_node_required = deployment_affinity_node_required;

        let registry_info = target.container_registry.registry_info();
        let registry_endpoint = registry_info.get_registry_endpoint(Some(target.kubernetes.cluster_name().as_str()));
        let registry_endpoint_host = registry_endpoint.host_str().unwrap_or_default();
        let (image_full, image_tag) = match &self.image_source {
            ImageSource::Registry { source } => {
                let repository: Cow<str> = if let Some(port) = registry_info
                    .get_registry_endpoint(Some(target.kubernetes.cluster_name().as_str()))
                    .port()
                {
                    format!("{registry_endpoint_host}:{port}").into()
                } else {
                    registry_endpoint_host.into()
                };

                let (_, image_name, image_tag, _) = source
                    .compute_cluster_container_registry_url_with_image_name_and_image_tag(
                        self.long_id(),
                        target.kubernetes.long_id(),
                        &target.kubernetes.advanced_settings().registry_mirroring_mode,
                        target.container_registry.registry_info(),
                    );
                let image_full = format!("{repository}/{image_name}:{image_tag}");
                (image_full, image_tag)
            }
            ImageSource::Build { source } => (source.image.full_image_name_with_tag(), source.image.tag.clone()),
        };

        JobTeraContext {
            organization_long_id: environment.organization_long_id,
            project_long_id: environment.project_long_id,
            environment_short_id: to_short_id(&environment.long_id),
            environment_long_id: environment.long_id,
            deployment_id: self.deployment_id.to_string(),
            cluster: ClusterTeraContext::from(kubernetes),
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
                cpu_request_in_milli: self.cpu_request_in_milli.to_string(),
                cpu_limit_in_milli: self.cpu_limit_in_milli.to_string(),
                ram_request_in_mib: self.ram_request_in_mib.to_string(),
                ram_limit_in_mib: self.ram_limit_in_mib.to_string(),
                default_port: self.default_port,
                max_nb_restart: self.max_nb_restart,
                max_duration_in_sec: self.max_duration.as_secs(),
                with_rbac: matches!(self.schedule.lifecycle_type(), Some(LifecycleType::TERRAFORM)),
                cronjob_schedule: match &self.schedule {
                    JobSchedule::OnStart { .. } | JobSchedule::OnPause { .. } | JobSchedule::OnDelete { .. } => None,
                    JobSchedule::Cron { schedule, .. } => Some(schedule.clone()),
                },
                cronjob_timezone: match &self.schedule {
                    JobSchedule::OnStart { .. } | JobSchedule::OnPause { .. } | JobSchedule::OnDelete { .. } => None,
                    JobSchedule::Cron { timezone, .. } => Some(timezone.clone()),
                },
                readiness_probe: self.readiness_probe.clone(),
                liveness_probe: self.liveness_probe.clone(),
                advanced_settings,
            },
            registry: match &self.image_source {
                ImageSource::Registry { source } => registry_info.get_registry_docker_json_config(DockerRegistryInfo {
                    registry_name: Some(kubernetes.cluster_name()), // TODO(benjaminch): this is a bit of a hack, considering registry name will be the same as cluster one, it should be the case, but worth doing it better
                    repository_name: None,
                    image_name: Some(source.image.to_string()),
                }),
                ImageSource::Build { source } => registry_info.get_registry_docker_json_config(DockerRegistryInfo {
                    registry_name: Some(kubernetes.cluster_name()), // TODO(benjaminch): this is a bit of a hack, considering registry name will be the same as cluster one, it should be the case, but worth doing it better
                    repository_name: Some(source.image.repository_name().to_string()),
                    image_name: Some(source.image.name()),
                }),
            }
            .as_ref()
            .map(|docker_json| RegistryTeraContext {
                secret_name: format!("{}-registry", self.kube_name()),
                docker_json_config: Some(docker_json.to_string()),
            }),
            environment_variables: self.environment_variables.clone(),
            mounted_files: self.mounted_files.clone().into_iter().collect::<Vec<_>>(),
            resource_expiration_in_seconds: Some(kubernetes.advanced_settings().pleco_resources_ttl),
            annotations_group: self.annotations_group.clone(),
            labels_group: self.labels_group.clone(),
        }
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
        self.workspace_directory.to_str().unwrap_or("")
    }

    pub fn should_delete_shared_registry(&self) -> bool {
        self.should_delete_shared_registry
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

    fn get_environment_variables(&self) -> Vec<EnvironmentVariable> {
        self.environment_variables.clone()
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
pub(crate) struct ServiceTeraContext {
    pub(crate) short_id: String,
    pub(crate) long_id: Uuid,
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) user_unsafe_name: String,
    pub(crate) image_full: String,
    pub(crate) image_tag: String,
    pub(crate) command_args: Vec<String>,
    pub(crate) entrypoint: Option<String>,
    pub(crate) cpu_request_in_milli: String,
    pub(crate) cpu_limit_in_milli: String,
    pub(crate) ram_request_in_mib: String,
    pub(crate) ram_limit_in_mib: String,
    pub(crate) default_port: Option<u16>,
    pub(crate) max_nb_restart: u32,
    pub(crate) max_duration_in_sec: u64,
    pub(crate) with_rbac: bool,
    pub(crate) cronjob_schedule: Option<String>,
    pub(crate) cronjob_timezone: Option<String>,
    pub(crate) readiness_probe: Option<Probe>,
    pub(crate) liveness_probe: Option<Probe>,
    pub(crate) advanced_settings: JobAdvancedSettings,
}

#[derive(Serialize, Debug, Clone)]
pub(crate) struct JobTeraContext {
    pub(crate) organization_long_id: Uuid,
    pub(crate) project_long_id: Uuid,
    pub(crate) environment_short_id: String,
    pub(crate) environment_long_id: Uuid,
    pub(crate) deployment_id: String,
    pub(crate) cluster: ClusterTeraContext,
    pub(crate) namespace: String,
    pub(crate) service: ServiceTeraContext,
    pub(crate) registry: Option<RegistryTeraContext>,
    pub(crate) environment_variables: Vec<EnvironmentVariable>,
    pub(crate) mounted_files: Vec<MountedFile>,
    pub(crate) resource_expiration_in_seconds: Option<i32>,
    pub(crate) annotations_group: AnnotationsGroupTeraContext,
    pub(crate) labels_group: LabelsGroupTeraContext,
}

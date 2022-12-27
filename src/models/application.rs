use crate::build_platform::Build;
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::models::{EnvironmentVariable, EnvironmentVariableDataTemplate, Storage};
use crate::cloud_provider::service::{Action, Service, ServiceType};
use crate::cloud_provider::utilities::sanitize_name;
use crate::deployment_action::DeploymentAction;
use crate::events::{EventDetails, Stage, Transmitter};
use crate::io_models::application::{AdvancedSettingsProbeType, ApplicationAdvancedSettings, Port};
use crate::io_models::context::Context;

use crate::models::types::{CloudProvider, ToTeraContext};
use crate::utilities::to_short_id;
use itertools::Itertools;
use std::marker::PhantomData;
use tera::Context as TeraContext;
use uuid::Uuid;

#[derive(thiserror::Error, Debug)]
pub enum ApplicationError {
    #[error("Application invalid configuration: {0}")]
    InvalidConfig(String),
}

pub struct Application<T: CloudProvider> {
    _marker: PhantomData<T>,
    pub(super) mk_event_details: Box<dyn Fn(Stage) -> EventDetails>,
    pub(super) id: String,
    pub(super) long_id: Uuid,
    pub(super) action: Action,
    pub(super) name: String,
    pub(super) ports: Vec<Port>,
    pub(super) total_cpus: String,
    pub(super) cpu_burst: String,
    pub(super) total_ram_in_mib: u32,
    pub(super) min_instances: u32,
    pub(super) max_instances: u32,
    pub(super) build: Build,
    pub(super) storage: Vec<Storage<T::StorageTypes>>,
    pub(super) environment_variables: Vec<EnvironmentVariable>,
    pub(super) advanced_settings: ApplicationAdvancedSettings,
    pub(super) _extra_settings: T::AppExtraSettings,
    pub(super) workspace_directory: String,
    pub(super) lib_root_directory: String,
}

// Here we define the common behavior among all providers
impl<T: CloudProvider> Application<T> {
    pub fn new(
        context: &Context,
        long_id: Uuid,
        action: Action,
        name: &str,
        ports: Vec<Port>,
        total_cpus: String,
        cpu_burst: String,
        total_ram_in_mib: u32,
        min_instances: u32,
        max_instances: u32,
        build: Build,
        storage: Vec<Storage<T::StorageTypes>>,
        environment_variables: Vec<EnvironmentVariable>,
        advanced_settings: ApplicationAdvancedSettings,
        extra_settings: T::AppExtraSettings,
        mk_event_details: impl Fn(Transmitter) -> EventDetails,
    ) -> Result<Self, ApplicationError> {
        // TODO: Check that the information provided are coherent

        let workspace_directory = crate::fs::workspace_directory(
            context.workspace_root_dir(),
            context.execution_id(),
            format!("applications/{}", long_id),
        )
        .map_err(|_| ApplicationError::InvalidConfig("Can't create workspace directory".to_string()))?;

        let event_details = mk_event_details(Transmitter::Application(long_id, name.to_string()));
        let mk_event_details = move |stage: Stage| EventDetails::clone_changing_stage(event_details.clone(), stage);
        Ok(Self {
            _marker: PhantomData,
            mk_event_details: Box::new(mk_event_details),
            id: to_short_id(&long_id),
            long_id,
            action,
            name: name.to_string(),
            ports,
            total_cpus,
            cpu_burst,
            total_ram_in_mib,
            min_instances,
            max_instances,
            build,
            storage,
            environment_variables,
            advanced_settings,
            _extra_settings: extra_settings,
            workspace_directory,
            lib_root_directory: context.lib_root_dir().to_string(),
        })
    }

    pub fn helm_selector(&self) -> Option<String> {
        Some(self.selector())
    }

    pub fn helm_release_name(&self) -> String {
        crate::string::cut(format!("application-{}-{}", self.id(), self.id()), 50)
    }

    pub fn helm_chart_dir(&self) -> String {
        format!("{}/{}/charts/q-application", self.lib_root_directory, T::lib_directory_name(),)
    }

    fn public_ports(&self) -> impl Iterator<Item = &Port> + '_ {
        self.ports.iter().filter(|port| port.publicly_accessible)
    }

    pub(super) fn default_tera_context(&self, kubernetes: &dyn Kubernetes, environment: &Environment) -> TeraContext {
        let mut context = TeraContext::new();
        context.insert("id", self.id());
        context.insert("long_id", &self.long_id);
        context.insert("owner_id", environment.owner_id.as_str());
        context.insert("project_id", environment.project_id.as_str());
        context.insert("project_long_id", &environment.project_long_id);
        context.insert("organization_id", environment.organization_id.as_str());
        context.insert("organization_long_id", &environment.organization_long_id);
        context.insert("environment_id", environment.id.as_str());
        context.insert("environment_long_id", &environment.long_id);
        context.insert("region", kubernetes.region());
        context.insert("zone", kubernetes.zone());
        context.insert("name", self.name());
        context.insert("sanitized_name", &self.sanitized_name());
        context.insert("namespace", environment.namespace());
        context.insert("cluster_name", kubernetes.name());
        context.insert("total_cpus", &self.total_cpus());
        context.insert("total_ram_in_mib", &self.total_ram_in_mib());
        context.insert("min_instances", &self.min_instances());
        context.insert("max_instances", &self.max_instances());

        context.insert(
            "security_service_account_name",
            &self.advanced_settings.security_service_account_name,
        );

        context.insert(
            "hpa_cpu_average_utilization_percent",
            &self.advanced_settings.hpa_cpu_average_utilization_percent,
        );

        if let Some(default_port) = self.ports.iter().find(|p| p.is_default) {
            context.insert("is_private_port", &true);
            context.insert("private_port", &default_port.port);
        } else {
            context.insert("is_private_port", &false);
        }

        context.insert("version", &self.commit_id());

        let commit_id = self.build.image.commit_id.as_str();
        context.insert("helm_app_version", &commit_id[..7]);
        context.insert("image_name_with_tag", &self.build.image.full_image_name_with_tag());

        let mut liveness_probe_initial_delay_seconds = self.advanced_settings.liveness_probe_initial_delay_seconds;
        let mut readiness_probe_initial_delay_seconds = self.advanced_settings.readiness_probe_initial_delay_seconds;

        if self.advanced_settings.deployment_delay_start_time_sec
            > self.advanced_settings.liveness_probe_initial_delay_seconds
            || self.advanced_settings.deployment_delay_start_time_sec
                > self.advanced_settings.readiness_probe_initial_delay_seconds
        {
            // note deployment_delay_start_time_sec is deprecated but we can keep using it to avoid breaking users apps
            // if the value is greater than `liveness_probe_initial_delay_seconds` or `readiness_probe_initial_delay_seconds` then we use it
            liveness_probe_initial_delay_seconds = self.advanced_settings.deployment_delay_start_time_sec;
            readiness_probe_initial_delay_seconds = self.advanced_settings.deployment_delay_start_time_sec;
        }

        context.insert("liveness_probe_initial_delay_seconds", &liveness_probe_initial_delay_seconds);
        context.insert("readiness_probe_initial_delay_seconds", &readiness_probe_initial_delay_seconds);
        context.insert(
            "liveness_probe_http_get_path",
            &self.advanced_settings.liveness_probe_http_get_path,
        );
        context.insert(
            "readiness_probe_http_get_path",
            &self.advanced_settings.readiness_probe_http_get_path,
        );
        context.insert(
            "liveness_probe_period_seconds",
            &self.advanced_settings.liveness_probe_period_seconds,
        );
        context.insert(
            "readiness_probe_period_seconds",
            &self.advanced_settings.readiness_probe_period_seconds,
        );
        context.insert(
            "liveness_probe_timeout_seconds",
            &self.advanced_settings.liveness_probe_timeout_seconds,
        );
        context.insert(
            "readiness_probe_timeout_seconds",
            &self.advanced_settings.readiness_probe_timeout_seconds,
        );
        context.insert(
            "liveness_probe_success_threshold",
            &self.advanced_settings.liveness_probe_success_threshold,
        );
        context.insert(
            "readiness_probe_success_threshold",
            &self.advanced_settings.readiness_probe_success_threshold,
        );
        context.insert(
            "liveness_probe_failure_threshold",
            &self.advanced_settings.liveness_probe_failure_threshold,
        );
        context.insert(
            "readiness_probe_failure_threshold",
            &self.advanced_settings.readiness_probe_failure_threshold,
        );

        match self.advanced_settings.readiness_probe_type {
            AdvancedSettingsProbeType::None => {
                context.insert("readiness_probe_enabled", &false);
                context.insert("readiness_probe_tcp_enabled", &false);
                context.insert("readiness_probe_http_enabled", &false);
            }
            AdvancedSettingsProbeType::Tcp => {
                context.insert("readiness_probe_enabled", &true);
                context.insert("readiness_probe_tcp_enabled", &true);
                context.insert("readiness_probe_http_enabled", &false);
            }
            AdvancedSettingsProbeType::Http => {
                context.insert("readiness_probe_enabled", &true);
                context.insert("readiness_probe_tcp_enabled", &false);
                context.insert("readiness_probe_http_enabled", &true);
            }
        };

        match self.advanced_settings.liveness_probe_type {
            AdvancedSettingsProbeType::None => {
                context.insert("liveness_probe_enabled", &false);
                context.insert("liveness_probe_tcp_enabled", &false);
                context.insert("liveness_probe_http_enabled", &false);
            }
            AdvancedSettingsProbeType::Tcp => {
                context.insert("liveness_probe_enabled", &true);
                context.insert("liveness_probe_tcp_enabled", &true);
                context.insert("liveness_probe_http_enabled", &false);
            }
            AdvancedSettingsProbeType::Http => {
                context.insert("liveness_probe_enabled", &true);
                context.insert("liveness_probe_tcp_enabled", &false);
                context.insert("liveness_probe_http_enabled", &true);
            }
        };

        let environment_variables = self
            .environment_variables
            .iter()
            .map(|ev| EnvironmentVariableDataTemplate {
                key: ev.key.clone(),
                value: ev.value.clone(),
            })
            .collect::<Vec<_>>();

        context.insert("environment_variables", &environment_variables);
        context.insert("ports", &self.ports);
        context.insert("is_registry_secret", &true);
        context.insert("registry_secret", self.build().image.registry_secret_name(kubernetes.kind()));

        context.insert(
            "resource_expiration_in_seconds",
            &kubernetes.advanced_settings().pleco_resources_ttl,
        );

        context
    }

    pub fn is_stateful(&self) -> bool {
        !self.storage.is_empty()
    }

    pub fn service_type(&self) -> ServiceType {
        ServiceType::Application
    }

    pub fn id(&self) -> &str {
        self.id.as_str()
    }

    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    pub fn commit_id(&self) -> String {
        self.build.image.commit_id.clone()
    }

    pub fn action(&self) -> &Action {
        &self.action
    }

    pub fn total_cpus(&self) -> String {
        self.total_cpus.to_string()
    }

    pub fn cpu_burst(&self) -> String {
        self.cpu_burst.to_string()
    }

    pub fn total_ram_in_mib(&self) -> u32 {
        self.total_ram_in_mib
    }

    pub fn min_instances(&self) -> u32 {
        self.min_instances
    }

    pub fn max_instances(&self) -> u32 {
        self.max_instances
    }

    pub fn publicly_accessible(&self) -> bool {
        self.public_ports().count() > 0
    }

    pub fn selector(&self) -> String {
        format!("appId={}", self.id())
    }

    pub fn build(&self) -> &Build {
        &self.build
    }

    pub fn build_mut(&mut self) -> &mut Build {
        &mut self.build
    }

    pub fn sanitized_name(&self) -> String {
        sanitize_name("app", self.id())
    }

    pub fn workspace_directory(&self) -> &str {
        &self.workspace_directory
    }
}

impl<T: CloudProvider> Service for Application<T> {
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
        self.sanitized_name()
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

    fn as_service_mut(&mut self) -> &mut dyn Service {
        self
    }

    fn build(&self) -> Option<&Build> {
        if self.action() == &Action::Create {
            Some(self.build())
        } else {
            None
        }
    }

    fn build_mut(&mut self) -> Option<&mut Build> {
        if self.action() == &Action::Create {
            Some(self.build_mut())
        } else {
            None
        }
    }
}

pub trait ApplicationService: Service + DeploymentAction + ToTeraContext {
    fn get_build(&self) -> &Build;
    fn get_build_mut(&mut self) -> &mut Build;
    fn public_ports(&self) -> Vec<&Port>;
    fn advanced_settings(&self) -> &ApplicationAdvancedSettings;
    fn startup_timeout(&self) -> std::time::Duration {
        let settings = self.advanced_settings();
        let readiness_probe_timeout = settings.readiness_probe_initial_delay_seconds
            + ((settings.readiness_probe_timeout_seconds + settings.readiness_probe_period_seconds)
                * settings.readiness_probe_failure_threshold);
        let liveness_probe_timeout = settings.liveness_probe_initial_delay_seconds
            + ((settings.liveness_probe_timeout_seconds + settings.liveness_probe_period_seconds)
                * settings.liveness_probe_failure_threshold);
        let probe_timeout = std::cmp::max(readiness_probe_timeout, liveness_probe_timeout);
        let startup_timeout = std::cmp::max(probe_timeout /* * 10 rolling restart percent */, 60 * 10);
        std::time::Duration::from_secs(startup_timeout as u64)
    }

    fn as_deployment_action(&self) -> &dyn DeploymentAction;
}

impl<T: CloudProvider> ApplicationService for Application<T>
where
    Application<T>: Service + ToTeraContext + DeploymentAction,
{
    fn get_build(&self) -> &Build {
        self.build()
    }

    fn get_build_mut(&mut self) -> &mut Build {
        self.build_mut()
    }

    fn public_ports(&self) -> Vec<&Port> {
        self.public_ports().collect_vec()
    }

    fn advanced_settings(&self) -> &ApplicationAdvancedSettings {
        &self.advanced_settings
    }

    fn as_deployment_action(&self) -> &dyn DeploymentAction {
        self
    }
}

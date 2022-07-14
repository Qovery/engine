use crate::build_platform::Build;
use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::models::{EnvironmentVariable, EnvironmentVariableDataTemplate, Storage};
use crate::cloud_provider::service::{delete_stateless_service, scale_down_application};
use crate::cloud_provider::service::{
    deploy_user_stateless_service, Action, Create, Delete, Helm, Pause, Service, ServiceType,
};
use crate::cloud_provider::utilities::{print_action, sanitize_name};
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::kubectl::ScalingKind::{Deployment, Statefulset};
use crate::deployment_report::application::reporter::ApplicationDeploymentReporter;
use crate::deployment_report::execute_long_deployment;
use crate::errors::EngineError;
use crate::events::{EnvironmentStep, EventDetails, Stage, ToTransmitter, Transmitter};
use crate::io_models::{
    ApplicationAdvancedSettings, ApplicationAdvancedSettingsProbeType, Context, Listen, Listener, Listeners, Port,
    QoveryIdentifier,
};
use crate::logger::Logger;
use crate::models::types::{CloudProvider, ToTeraContext};
use crate::utilities::to_short_id;
use function_name::named;
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
    pub(super) context: Context,
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
    pub(super) listeners: Listeners,
    pub(super) logger: Box<dyn Logger>,
    pub(super) advanced_settings: ApplicationAdvancedSettings,
    pub(super) _extra_settings: T::AppExtraSettings,
}

// Here we define the common behavior among all providers
impl<T: CloudProvider> Application<T> {
    pub fn new(
        context: Context,
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
        listeners: Listeners,
        logger: Box<dyn Logger>,
    ) -> Result<Self, ApplicationError> {
        // TODO: Check that the information provided are coherent

        Ok(Self {
            _marker: PhantomData,
            context,
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
            listeners,
            logger,
            advanced_settings,
            _extra_settings: extra_settings,
        })
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
        context.insert("region", kubernetes.region().as_str());
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
            "hpa_cpu_average_utilization_percent",
            &self.advanced_settings.hpa_cpu_average_utilization_percent,
        );

        if let Some(private_port) = self.public_port() {
            context.insert("is_private_port", &true);
            context.insert("private_port", &private_port);
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
            ApplicationAdvancedSettingsProbeType::None => {
                context.insert("readiness_probe_enabled", &false);
                context.insert("readiness_probe_tcp_enabled", &false);
                context.insert("readiness_probe_http_enabled", &false);
            }
            ApplicationAdvancedSettingsProbeType::Tcp => {
                context.insert("readiness_probe_enabled", &true);
                context.insert("readiness_probe_tcp_enabled", &true);
                context.insert("readiness_probe_http_enabled", &false);
            }
            ApplicationAdvancedSettingsProbeType::Http => {
                context.insert("readiness_probe_enabled", &true);
                context.insert("readiness_probe_tcp_enabled", &false);
                context.insert("readiness_probe_http_enabled", &true);
            }
        };

        match self.advanced_settings.liveness_probe_type {
            ApplicationAdvancedSettingsProbeType::None => {
                context.insert("liveness_probe_enabled", &false);
                context.insert("liveness_probe_tcp_enabled", &false);
                context.insert("liveness_probe_http_enabled", &false);
            }
            ApplicationAdvancedSettingsProbeType::Tcp => {
                context.insert("liveness_probe_enabled", &true);
                context.insert("liveness_probe_tcp_enabled", &true);
                context.insert("liveness_probe_http_enabled", &false);
            }
            ApplicationAdvancedSettingsProbeType::Http => {
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

        if self.context.resource_expiration_in_seconds().is_some() {
            context.insert("resource_expiration_in_seconds", &self.context.resource_expiration_in_seconds())
        }

        context
    }

    pub fn is_stateful(&self) -> bool {
        !self.storage.is_empty()
    }

    pub fn context(&self) -> &Context {
        &self.context
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

    pub fn public_port(&self) -> Option<u16> {
        self.ports
            .iter()
            .find(|port| port.publicly_accessible)
            .map(|port| port.port as u16)
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
        self.public_port().is_some()
    }

    pub fn logger(&self) -> &dyn Logger {
        &*self.logger
    }

    pub fn selector(&self) -> Option<String> {
        Some(format!("appId={}", self.id()))
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

    pub(crate) fn get_event_details(&self, stage: Stage) -> EventDetails {
        let context = self.context();
        EventDetails::new(
            None,
            QoveryIdentifier::from(context.organization_id().to_string()),
            QoveryIdentifier::from(context.cluster_id().to_string()),
            QoveryIdentifier::from(context.execution_id().to_string()),
            None,
            stage,
            self.to_transmitter(),
        )
    }
}

// Traits implementations
impl<T: CloudProvider> ToTransmitter for Application<T> {
    fn to_transmitter(&self) -> Transmitter {
        Transmitter::Application(self.id.to_string(), self.name.to_string(), self.commit_id())
    }
}

impl<T: CloudProvider> Listen for Application<T> {
    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }
}

impl<T: CloudProvider> Service for Application<T>
where
    Application<T>: ToTeraContext,
{
    fn context(&self) -> &Context {
        self.context()
    }

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

    fn application_advanced_settings(&self) -> Option<ApplicationAdvancedSettings> {
        Some(self.advanced_settings.clone())
    }

    fn version(&self) -> String {
        self.commit_id()
    }

    fn action(&self) -> &Action {
        self.action()
    }

    fn private_port(&self) -> Option<u16> {
        self.public_port()
    }

    fn total_cpus(&self) -> String {
        self.total_cpus()
    }

    fn cpu_burst(&self) -> String {
        self.cpu_burst()
    }

    fn total_ram_in_mib(&self) -> u32 {
        self.total_ram_in_mib()
    }

    fn min_instances(&self) -> u32 {
        self.min_instances()
    }

    fn max_instances(&self) -> u32 {
        self.max_instances()
    }

    fn publicly_accessible(&self) -> bool {
        self.publicly_accessible()
    }

    fn tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        self.to_tera_context(target)
    }

    fn logger(&self) -> &dyn Logger {
        self.logger()
    }

    fn selector(&self) -> Option<String> {
        self.selector()
    }

    fn as_service(&self) -> &dyn Service {
        self
    }
}

impl<T: CloudProvider> Helm for Application<T> {
    fn helm_selector(&self) -> Option<String> {
        self.selector()
    }

    fn helm_release_name(&self) -> String {
        crate::string::cut(format!("application-{}-{}", self.id(), self.id()), 50)
    }

    fn helm_chart_dir(&self) -> String {
        format!(
            "{}/{}/charts/q-application",
            self.context.lib_root_dir(),
            T::lib_directory_name(),
        )
    }

    fn helm_chart_values_dir(&self) -> String {
        String::new()
    }

    fn helm_chart_external_name_service_dir(&self) -> String {
        String::new()
    }
}

impl<T: CloudProvider> Create for Application<T>
where
    Application<T>: Service,
{
    #[named]
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        print_action(
            T::short_name(),
            "application",
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );

        execute_long_deployment(ApplicationDeploymentReporter::new(self, target, Action::Create), || {
            deploy_user_stateless_service(target, self)
        })
    }

    fn on_create_check(&self) -> Result<(), EngineError> {
        Ok(())
    }
}

impl<T: CloudProvider> Pause for Application<T>
where
    Application<T>: Service,
{
    #[named]
    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Pause));
        print_action(
            T::short_name(),
            "application",
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );

        execute_long_deployment(ApplicationDeploymentReporter::new(self, target, Action::Pause), || {
            scale_down_application(target, self, 0, if self.is_stateful() { Statefulset } else { Deployment })
        })
    }

    fn on_pause_check(&self) -> Result<(), EngineError> {
        Ok(())
    }
}

impl<T: CloudProvider> Delete for Application<T>
where
    Application<T>: Service,
{
    #[named]
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Delete));
        print_action(
            T::short_name(),
            "application",
            function_name!(),
            self.name(),
            event_details.clone(),
            self.logger(),
        );

        execute_long_deployment(ApplicationDeploymentReporter::new(self, target, Action::Delete), || {
            delete_stateless_service(target, self, event_details.clone())
        })
    }

    fn on_delete_check(&self) -> Result<(), EngineError> {
        Ok(())
    }
}

pub trait ApplicationService: Service + Create + Pause + Delete + Listen {
    fn get_build(&self) -> &Build;
    fn get_build_mut(&mut self) -> &mut Build;
    fn exec_action(&self, deployment_target: &DeploymentTarget) -> Result<(), EngineError> {
        match self.action() {
            Action::Create => self.on_create(deployment_target),
            Action::Delete => self.on_delete(deployment_target),
            Action::Pause => self.on_pause(deployment_target),
            Action::Nothing => Ok(()),
        }
    }

    fn exec_check_action(&self) -> Result<(), EngineError> {
        match self.action() {
            Action::Create => self.on_create_check(),
            Action::Delete => self.on_delete_check(),
            Action::Pause => self.on_pause_check(),
            Action::Nothing => Ok(()),
        }
    }
}

impl<T: CloudProvider> ApplicationService for Application<T>
where
    Application<T>: Service,
{
    fn get_build(&self) -> &Build {
        self.build()
    }

    fn get_build_mut(&mut self) -> &mut Build {
        self.build_mut()
    }
}

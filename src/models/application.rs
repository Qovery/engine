use crate::build_platform::Build;
use crate::cloud_provider::models::{EnvironmentVariable, Storage};
use crate::cloud_provider::service::{delete_stateless_service, scale_down_application};
use crate::cloud_provider::service::{
    deploy_stateless_service_error, deploy_user_stateless_service, send_progress_on_long_task, Action, Create, Delete,
    Helm, Pause, Service, ServiceType, StatelessService,
};
use crate::cloud_provider::utilities::{print_action, sanitize_name};
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::helm::Timeout;
use crate::cmd::kubectl::ScalingKind::{Deployment, Statefulset};
use crate::errors::EngineError;
use crate::events::{EnvironmentStep, EventDetails, Stage, ToTransmitter, Transmitter};
use crate::io_models::{Context, Listen, Listener, Listeners, Port, QoveryIdentifier};
use crate::logger::Logger;
use crate::models::types::CloudProvider;
use function_name::named;
use std::marker::PhantomData;
use tera::Context as TeraContext;

#[derive(thiserror::Error, Debug)]
pub enum ApplicationError {
    #[error("Application invalid configuration: {0}")]
    InvalidConfig(String),
}

pub struct Application<T: CloudProvider> {
    _marker: PhantomData<T>,
    pub(crate) context: Context,
    pub(crate) id: String,
    pub(crate) action: Action,
    pub(crate) name: String,
    pub(crate) ports: Vec<Port>,
    pub(crate) total_cpus: String,
    pub(crate) cpu_burst: String,
    pub(crate) total_ram_in_mib: u32,
    pub(crate) min_instances: u32,
    pub(crate) max_instances: u32,
    pub(crate) start_timeout_in_seconds: u32,
    pub(crate) build: Build,
    pub(crate) storage: Vec<Storage<T::StorageTypes>>,
    pub(crate) environment_variables: Vec<EnvironmentVariable>,
    pub(crate) listeners: Listeners,
    pub(crate) logger: Box<dyn Logger>,
    pub(crate) _extra_settings: T::AppExtraSettings,
}

// Here we define the common behavior among all providers
impl<T: CloudProvider> Application<T> {
    pub fn new(
        context: Context,
        id: &str,
        action: Action,
        name: &str,
        ports: Vec<Port>,
        total_cpus: String,
        cpu_burst: String,
        total_ram_in_mib: u32,
        min_instances: u32,
        max_instances: u32,
        start_timeout_in_seconds: u32,
        build: Build,
        storage: Vec<Storage<T::StorageTypes>>,
        environment_variables: Vec<EnvironmentVariable>,
        extra_settings: T::AppExtraSettings,
        listeners: Listeners,
        logger: Box<dyn Logger>,
    ) -> Result<Self, ApplicationError> {
        // TODO: Check that the information provided are coherent

        Ok(Self {
            _marker: PhantomData,
            context,
            id: id.to_string(),
            action,
            name: name.to_string(),
            ports,
            total_cpus,
            cpu_burst,
            total_ram_in_mib,
            min_instances,
            max_instances,
            start_timeout_in_seconds,
            build,
            storage,
            environment_variables,
            listeners,
            logger,
            _extra_settings: extra_settings,
        })
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

    pub fn start_timeout(&self) -> u32 {
        (self.start_timeout_in_seconds + 10) * 4
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

    pub fn sanitize_name(&self) -> String {
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
        Transmitter::Application(self.id.to_string(), self.name.to_string())
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

pub(crate) trait ToTeraContext {
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError>;
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

    fn name(&self) -> &str {
        self.name()
    }

    fn sanitized_name(&self) -> String {
        self.sanitize_name()
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

    fn start_timeout(&self) -> Timeout<u32> {
        Timeout::Value(self.start_timeout())
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
            T::helm_directory_name(),
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
        send_progress_on_long_task(self, crate::cloud_provider::service::Action::Create, || {
            deploy_user_stateless_service(target, self)
        })
    }

    fn on_create_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    #[named]
    fn on_create_error(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        print_action(
            T::short_name(),
            "application",
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );

        send_progress_on_long_task(self, crate::cloud_provider::service::Action::Create, || {
            deploy_stateless_service_error(target, self)
        })
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

        send_progress_on_long_task(self, crate::cloud_provider::service::Action::Pause, || {
            scale_down_application(target, self, 0, if self.is_stateful() { Statefulset } else { Deployment })
        })
    }

    fn on_pause_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    #[named]
    fn on_pause_error(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Pause));
        print_action(
            T::short_name(),
            "application",
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );

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

        send_progress_on_long_task(self, crate::cloud_provider::service::Action::Delete, || {
            delete_stateless_service(target, self, event_details.clone())
        })
    }

    fn on_delete_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    #[named]
    fn on_delete_error(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Delete));
        print_action(
            T::short_name(),
            "application",
            function_name!(),
            self.name(),
            event_details.clone(),
            self.logger(),
        );

        send_progress_on_long_task(self, crate::cloud_provider::service::Action::Delete, || {
            delete_stateless_service(target, self, event_details.clone())
        })
    }
}

impl<T: CloudProvider> StatelessService for Application<T>
where
    Application<T>: Service,
{
    fn as_stateless_service(&self) -> &dyn StatelessService {
        self
    }
}

pub trait IApplication: StatelessService {
    fn get_build(&self) -> &Build;
    fn get_build_mut(&mut self) -> &mut Build;
}

impl<T: CloudProvider> IApplication for Application<T>
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

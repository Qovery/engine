use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::models::{EnvironmentVariable, Storage, StorageDataTemplate};
use crate::cloud_provider::service::{Action, Service, ServiceType};
use crate::deployment_action::DeploymentAction;
use crate::events::{EventDetails, Stage, Transmitter};
use crate::io_models::application::Port;
use crate::io_models::container::{ContainerAdvancedSettings, Registry};
use crate::io_models::context::Context;
use crate::io_models::progress_listener::{Listener, Listeners};
use crate::io_models::QoveryIdentifier;
use crate::logger::Logger;
use crate::models::types::{CloudProvider, ToTeraContext};
use crate::utilities::to_short_id;
use serde::Serialize;
use std::marker::PhantomData;
use uuid::Uuid;

#[derive(thiserror::Error, Debug)]
pub enum ContainerError {
    #[error("Container invalid configuration: {0}")]
    InvalidConfig(String),
}

pub struct Container<T: CloudProvider> {
    _marker: PhantomData<T>,
    pub(super) context: Context,
    pub(super) id: String,
    pub(super) long_id: Uuid,
    pub(super) name: String,
    pub(super) action: Action,
    pub(super) registry: Registry,
    pub(super) image: String,
    pub(super) tag: String,
    pub(super) command_args: Vec<String>,
    pub(super) entrypoint: Option<String>,
    pub(super) cpu_request_in_mili: u32,
    pub(super) cpu_limit_in_mili: u32,
    pub(super) ram_request_in_mib: u32,
    pub(super) ram_limit_in_mib: u32,
    pub(super) min_instances: u32,
    pub(super) max_instances: u32,
    pub(super) ports: Vec<Port>,
    pub(super) storages: Vec<Storage<T::StorageTypes>>,
    pub(super) environment_variables: Vec<EnvironmentVariable>,
    pub(super) listeners: Listeners,
    pub(super) logger: Box<dyn Logger>,
    pub(super) advanced_settings: ContainerAdvancedSettings,
    pub(super) _extra_settings: T::AppExtraSettings,
}

// Here we define the common behavior among all providers
impl<T: CloudProvider> Container<T> {
    pub fn new(
        context: Context,
        long_id: Uuid,
        name: String,
        action: Action,
        registry: Registry,
        image: String,
        tag: String,
        command_args: Vec<String>,
        entrypoint: Option<String>,
        cpu_request_in_mili: u32,
        cpu_limit_in_mili: u32,
        ram_request_in_mib: u32,
        ram_limit_in_mib: u32,
        min_instances: u32,
        max_instances: u32,
        ports: Vec<Port>,
        storages: Vec<Storage<T::StorageTypes>>,
        environment_variables: Vec<EnvironmentVariable>,
        advanced_settings: ContainerAdvancedSettings,
        extra_settings: T::AppExtraSettings,
        listeners: Listeners,
        logger: Box<dyn Logger>,
    ) -> Result<Self, ContainerError> {
        if min_instances > max_instances {
            return Err(ContainerError::InvalidConfig(
                "min_instances must be less or equal to max_instances".to_string(),
            ));
        }

        if min_instances == 0 {
            return Err(ContainerError::InvalidConfig(
                "min_instances must be greater than 0".to_string(),
            ));
        }

        if cpu_request_in_mili > cpu_limit_in_mili {
            return Err(ContainerError::InvalidConfig(
                "cpu_request_in_mili must be less or equal to cpu_limit_in_mili".to_string(),
            ));
        }

        if cpu_request_in_mili == 0 {
            return Err(ContainerError::InvalidConfig(
                "cpu_request_in_mili must be greater than 0".to_string(),
            ));
        }

        if ram_request_in_mib > ram_limit_in_mib {
            return Err(ContainerError::InvalidConfig(
                "ram_request_in_mib must be less or equal to ram_limit_in_mib".to_string(),
            ));
        }

        if ram_request_in_mib == 0 {
            return Err(ContainerError::InvalidConfig(
                "ram_request_in_mib must be greater than 0".to_string(),
            ));
        }

        Ok(Self {
            _marker: PhantomData,
            context,
            id: to_short_id(&long_id),
            long_id,
            action,
            name,
            registry,
            image,
            tag,
            command_args,
            entrypoint,
            cpu_request_in_mili,
            cpu_limit_in_mili,
            ram_request_in_mib,
            ram_limit_in_mib,
            min_instances,
            max_instances,
            ports,
            storages,
            environment_variables,
            listeners,
            logger,
            advanced_settings,
            _extra_settings: extra_settings,
        })
    }

    pub fn helm_selector(&self) -> Option<String> {
        Some(self.selector())
    }

    pub fn helm_release_name(&self) -> String {
        format!("container-{}", self.long_id)
    }

    pub fn helm_chart_dir(&self) -> String {
        format!("{}/{}/charts/q-container", self.context.lib_root_dir(), T::lib_directory_name(),)
    }

    fn public_port(&self) -> Option<u16> {
        self.ports
            .iter()
            .find(|port| port.publicly_accessible)
            .map(|port| port.port as u16)
    }

    pub(super) fn default_tera_context(
        &self,
        kubernetes: &dyn Kubernetes,
        environment: &Environment,
    ) -> ContainerTeraContext {
        let ctx = ContainerTeraContext {
            organization_long_id: environment.organization_long_id,
            project_long_id: environment.project_long_id,
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
                name: self.name.clone(),
                image_full: format!("{}/{}:{}", self.registry.url(), self.image, self.tag),
                image_tag: self.tag.clone(),
                commands: self.command_args.clone(),
                entrypoint: self.entrypoint.clone(),
                cpu_request_in_mili: format!("{}m", self.cpu_request_in_mili),
                cpu_limit_in_mili: format!("{}m", self.cpu_limit_in_mili),
                ram_request_in_mib: format!("{}Mi", self.ram_request_in_mib),
                ram_limit_in_mib: format!("{}Mi", self.ram_limit_in_mib),
                min_instances: self.min_instances,
                max_instances: self.max_instances,
                ports: self.ports.clone(),
                storages: vec![],
                advanced_settings: self.advanced_settings.clone(),
            },
            registry: None,
            environment_variables: self.environment_variables.clone(),
            resource_expiration_in_seconds: self.context.resource_expiration_in_seconds(),
        };

        ctx
    }

    pub fn is_stateful(&self) -> bool {
        !self.storages.is_empty()
    }

    pub fn context(&self) -> &Context {
        &self.context
    }

    pub fn service_type(&self) -> ServiceType {
        ServiceType::Container
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

    pub fn publicly_accessible(&self) -> bool {
        self.public_port().is_some()
    }

    pub fn image_with_tag(&self) -> String {
        format!("{}:{}", self.image, self.tag)
    }

    pub fn logger(&self) -> &dyn Logger {
        &*self.logger
    }

    pub fn selector(&self) -> String {
        format!("qovery.com/service-id={}", self.long_id)
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

impl<T: CloudProvider> Service for Container<T> {
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
        self.name.to_string()
    }

    fn version(&self) -> String {
        "1".to_string()
    }

    fn action(&self) -> &Action {
        self.action()
    }

    fn selector(&self) -> Option<String> {
        Some(self.selector())
    }

    fn logger(&self) -> &dyn Logger {
        self.logger()
    }

    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }

    fn to_transmitter(&self) -> Transmitter {
        Transmitter::Container(self.id.to_string(), self.name.to_string(), self.image_with_tag())
    }

    fn as_service(&self) -> &dyn Service {
        self
    }
}

pub trait ContainerService: Service + DeploymentAction + ToTeraContext {
    fn public_port(&self) -> Option<u16>;
    fn advanced_settings(&self) -> &ContainerAdvancedSettings;
}

impl<T: CloudProvider> ContainerService for Container<T>
where
    Container<T>: Service + ToTeraContext + DeploymentAction,
{
    fn public_port(&self) -> Option<u16> {
        self.public_port()
    }

    fn advanced_settings(&self) -> &ContainerAdvancedSettings {
        &self.advanced_settings
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
    pub(super) image_full: String,
    pub(super) image_tag: String,
    pub(super) commands: Vec<String>,
    pub(super) entrypoint: Option<String>,
    pub(super) cpu_request_in_mili: String,
    pub(super) cpu_limit_in_mili: String,
    pub(super) ram_request_in_mib: String,
    pub(super) ram_limit_in_mib: String,
    pub(super) min_instances: u32,
    pub(super) max_instances: u32,
    pub(super) ports: Vec<Port>,
    pub(super) storages: Vec<StorageDataTemplate>,
    pub(super) advanced_settings: ContainerAdvancedSettings,
}

#[derive(Serialize, Debug, Clone)]
pub(super) struct RegistryTeraContext {
    pub(super) secret_name: String,
    pub(super) docker_json_config: String,
}

#[derive(Serialize, Debug, Clone)]
pub(super) struct ContainerTeraContext {
    pub(super) organization_long_id: Uuid,
    pub(super) project_long_id: Uuid,
    pub(super) environment_long_id: Uuid,
    pub(super) cluster: ClusterTeraContext,
    pub(super) namespace: String,
    pub(super) service: ServiceTeraContext,
    pub(super) registry: Option<RegistryTeraContext>,
    pub(super) environment_variables: Vec<EnvironmentVariable>,
    pub(super) resource_expiration_in_seconds: Option<u32>,
}

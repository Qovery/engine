use crate::cloud_provider::models::{EnvironmentVariable, Storage, StorageDataTemplate};
use crate::cloud_provider::service::{Action, Service, ServiceType};
use crate::cloud_provider::DeploymentTarget;
use crate::deployment_action::DeploymentAction;
use crate::events::{EventDetails, Stage, Transmitter};
use crate::io_models::application::Port;
use crate::io_models::container::{ContainerAdvancedSettings, Registry};
use crate::io_models::context::Context;
use crate::io_models::progress_listener::{Listener, Listeners};
use crate::io_models::QoveryIdentifier;
use crate::logger::Logger;
use crate::models::types::{CloudProvider, ToTeraContext};
use crate::string::cut;
use crate::utilities::to_short_id;
use itertools::Itertools;
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
    pub registry: Registry,
    pub image: String,
    pub tag: String,
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
    pub const QOVERY_MIRROR_REPOSITORY_NAME: &'static str = "qovery-mirror";

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
        format!("{}/common/charts/q-container", self.context.lib_root_dir())
    }

    fn kube_service_name(&self) -> String {
        format!("container-{}", to_short_id(&self.long_id))
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    fn public_port(&self) -> Option<u16> {
        self.ports
            .iter()
            .find(|port| port.publicly_accessible)
            .map(|port| port.port as u16)
    }

    pub(super) fn default_tera_context(&self, target: &DeploymentTarget) -> ContainerTeraContext {
        let environment = &target.environment;
        let kubernetes = &target.kubernetes;
        let registry_info = target.container_registry.registry_info();

        let ctx = ContainerTeraContext {
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
                    (registry_info.get_image_name)(Self::QOVERY_MIRROR_REPOSITORY_NAME),
                    self.tag_for_mirror()
                ),
                image_tag: self.tag_for_mirror(),
                command_args: self.command_args.clone(),
                entrypoint: self.entrypoint.clone(),
                cpu_request_in_mili: format!("{}m", self.cpu_request_in_mili),
                cpu_limit_in_mili: format!("{}m", self.cpu_limit_in_mili),
                ram_request_in_mib: format!("{}Mi", self.ram_request_in_mib),
                ram_limit_in_mib: format!("{}Mi", self.ram_limit_in_mib),
                min_instances: self.min_instances,
                max_instances: self.max_instances,
                ports: self.ports.clone(),
                default_port: self.ports.iter().find_or_first(|p| p.publicly_accessible).cloned(),
                storages: vec![],
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

    pub fn tag_for_mirror(&self) -> String {
        // A tag name must be valid ASCII and may contain lowercase and uppercase letters, digits, underscores, periods and dashes.
        // A tag name may not start with a period or a dash and may contain a maximum of 128 characters.
        cut(format!("{}.{}.{}", self.image.replace('/', "."), self.tag, self.long_id), 128)
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
    fn image_full(&self) -> String;
    fn kube_service_name(&self) -> String;
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
    pub(super) cpu_request_in_mili: String,
    pub(super) cpu_limit_in_mili: String,
    pub(super) ram_request_in_mib: String,
    pub(super) ram_limit_in_mib: String,
    pub(super) min_instances: u32,
    pub(super) max_instances: u32,
    pub(super) ports: Vec<Port>,
    pub(super) default_port: Option<Port>,
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
    pub(super) environment_short_id: String,
    pub(super) environment_long_id: Uuid,
    pub(super) cluster: ClusterTeraContext,
    pub(super) namespace: String,
    pub(super) service: ServiceTeraContext,
    pub(super) registry: Option<RegistryTeraContext>,
    pub(super) environment_variables: Vec<EnvironmentVariable>,
    pub(super) resource_expiration_in_seconds: Option<i32>,
}

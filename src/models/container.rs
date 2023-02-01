use crate::build_platform::Build;
use crate::cloud_provider::models::{
    EnvironmentVariable, InvalidPVCStorage, InvalidStatefulsetStorage, MountedFile, Storage, StorageDataTemplate,
};
use crate::cloud_provider::service::{get_service_statefulset_name_and_volumes, Action, Service, ServiceType};
use crate::cloud_provider::DeploymentTarget;
use crate::deployment_action::DeploymentAction;
use crate::errors::EngineError;
use crate::events::{EventDetails, Stage, Transmitter};
use crate::io_models::application::Port;
use crate::io_models::container::{ContainerAdvancedSettings, Registry};
use crate::io_models::context::Context;
use crate::kubers_utils::kube_get_resources_by_selector;
use crate::models::types::{CloudProvider, ToTeraContext};
use crate::runtime::block_on;
use crate::string::cut;
use crate::unit_conversion::extract_volume_size;
use crate::utilities::to_short_id;
use itertools::Itertools;
use k8s_openapi::api::core::v1::PersistentVolumeClaim;
use serde::Serialize;
use std::collections::BTreeSet;
use std::marker::PhantomData;
use uuid::Uuid;

#[derive(thiserror::Error, Debug)]
pub enum ContainerError {
    #[error("Container invalid configuration: {0}")]
    InvalidConfig(String),
}

pub struct Container<T: CloudProvider> {
    _marker: PhantomData<T>,
    pub(super) mk_event_details: Box<dyn Fn(Stage) -> EventDetails>,
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
    pub(super) mounted_files: BTreeSet<MountedFile>,
    pub(super) advanced_settings: ContainerAdvancedSettings,
    pub(super) _extra_settings: T::AppExtraSettings,
    pub(super) workspace_directory: String,
    pub(super) lib_root_directory: String,
}

pub fn get_mirror_repository_name(service_id: &Uuid) -> String {
    format!("qovery-mirror-{service_id}")
}

// Here we define the common behavior among all providers
impl<T: CloudProvider> Container<T> {
    pub fn new(
        context: &Context,
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
        mounted_files: BTreeSet<MountedFile>,
        advanced_settings: ContainerAdvancedSettings,
        extra_settings: T::AppExtraSettings,
        mk_event_details: impl Fn(Transmitter) -> EventDetails,
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

        let workspace_directory = crate::fs::workspace_directory(
            context.workspace_root_dir(),
            context.execution_id(),
            format!("containers/{long_id}"),
        )
        .map_err(|_| ContainerError::InvalidConfig("Can't create workspace directory".to_string()))?;

        let event_details = mk_event_details(Transmitter::Container(long_id, name.to_string()));
        let mk_event_details = move |stage: Stage| EventDetails::clone_changing_stage(event_details.clone(), stage);
        Ok(Self {
            _marker: PhantomData,
            mk_event_details: Box::new(mk_event_details),
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
            mounted_files,
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
        format!("container-{}", self.long_id)
    }

    pub fn helm_chart_dir(&self) -> String {
        format!("{}/common/charts/q-container", self.lib_root_directory)
    }

    fn kube_service_name(&self) -> String {
        format!("container-{}", to_short_id(&self.long_id))
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    fn public_ports(&self) -> impl Iterator<Item = &Port> + '_ {
        self.ports.iter().filter(|port| port.publicly_accessible)
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
                    (registry_info.get_image_name)(&get_mirror_repository_name(self.long_id())),
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
                default_port: self.ports.iter().find_or_first(|p| p.is_default).cloned(),
                storages: vec![],
                advanced_settings: self.advanced_settings.clone(),
            },
            registry: registry_info
                .registry_docker_json_config
                .as_ref()
                .map(|docker_json| RegistryTeraContext {
                    secret_name: format!("{}-registry", self.kube_service_name()),
                    docker_json_config: Some(docker_json.to_string()),
                }),
            environment_variables: self.environment_variables.clone(),
            mounted_files: self.mounted_files.clone().into_iter().collect::<Vec<_>>(),
            resource_expiration_in_seconds: Some(kubernetes.advanced_settings().pleco_resources_ttl),
        };

        ctx
    }

    pub fn is_stateful(&self) -> bool {
        !self.storages.is_empty()
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
        self.public_ports().count() > 0
    }

    pub fn image_with_tag(&self) -> String {
        format!("{}:{}", self.image, self.tag)
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

impl<T: CloudProvider> Service for Container<T> {
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
        None
    }

    fn build_mut(&mut self) -> Option<&mut Build> {
        None
    }
}

pub trait ContainerService: Service + DeploymentAction + ToTeraContext {
    fn public_ports(&self) -> Vec<&Port>;
    fn advanced_settings(&self) -> &ContainerAdvancedSettings;
    fn image_full(&self) -> String;
    fn kube_service_name(&self) -> String;
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

impl<T: CloudProvider> ContainerService for Container<T>
where
    Container<T>: Service + ToTeraContext + DeploymentAction,
{
    fn public_ports(&self) -> Vec<&Port> {
        self.public_ports().collect_vec()
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

    fn as_deployment_action(&self) -> &dyn DeploymentAction {
        self
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
    pub(super) docker_json_config: Option<String>,
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
    pub(super) mounted_files: Vec<MountedFile>,
    pub(super) resource_expiration_in_seconds: Option<i32>,
}

pub fn get_container_with_invalid_storage_size<T: CloudProvider>(
    container: &Container<T>,
    kube_client: &kube::Client,
    namespace: &str,
    event_details: &EventDetails,
) -> Result<Option<InvalidStatefulsetStorage>, Box<EngineError>> {
    match !container.is_stateful() {
        true => Ok(None),
        false => {
            let selector = Container::selector(container);
            let (statefulset_name, statefulset_volumes) =
                get_service_statefulset_name_and_volumes(kube_client, namespace, &selector, event_details)?;
            let storage_err = Box::new(EngineError::new_service_missing_storage(
                event_details.clone(),
                &container.long_id,
            ));
            let volumes = match statefulset_volumes {
                None => return Err(storage_err),
                Some(volumes) => volumes,
            };
            let mut invalid_storage = InvalidStatefulsetStorage {
                service_type: Container::service_type(container),
                service_id: container.long_id,
                statefulset_selector: selector,
                statefulset_name,
                invalid_pvcs: vec![],
            };

            for volume in volumes {
                if let Some(spec) = &volume.spec {
                    if let Some(resources) = &spec.resources {
                        if let (Some(requests), Some(volume_name)) = (&resources.requests, &volume.metadata.name) {
                            // in order to compare volume size from engine request to effective size in kube, we must get the  effective size
                            let size = extract_volume_size(requests["storage"].0.to_string()).map_err(|e| {
                                Box::new(EngineError::new_cannot_parse_string(
                                    event_details.clone(),
                                    &requests["storage"].0,
                                    e,
                                ))
                            })?;
                            if let Some(storage) = container
                                .storages
                                .iter()
                                .find(|storage| volume_name == &storage.long_id.to_string())
                            {
                                if storage.size_in_gib > size {
                                    // if volume size in request is bigger than effective size we get related PVC to get its infos
                                    if let Some(pvc) =
                                        block_on(kube_get_resources_by_selector::<PersistentVolumeClaim>(
                                            kube_client,
                                            namespace,
                                            &format!("qovery.com/disk-id={}", storage.long_id),
                                        ))
                                        .map_err(|e| {
                                            EngineError::new_k8s_cannot_get_pvcs(event_details.clone(), namespace, e)
                                        })?
                                        .items
                                        .first()
                                    {
                                        if let Some(pvc_name) = &pvc.metadata.name {
                                            invalid_storage.invalid_pvcs.push(InvalidPVCStorage {
                                                pvc_name: pvc_name.to_string(),
                                                required_disk_size_in_gib: storage.size_in_gib,
                                            })
                                        }
                                    };
                                }

                                if storage.size_in_gib < size {
                                    return Err(Box::new(EngineError::new_invalid_engine_payload(
                                        event_details.clone(),
                                        format!(
                                            "new storage size ({}) should be equal or greater than actual size ({})",
                                            storage.size_in_gib, size
                                        )
                                        .as_str(),
                                    )));
                                }
                            }
                        }
                    }
                }
            }

            match invalid_storage.invalid_pvcs.is_empty() {
                true => Ok(None),
                false => Ok(Some(invalid_storage)),
            }
        }
    }
}

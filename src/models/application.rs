use crate::build_platform::Build;
use crate::cloud_provider::models::{
    EnvironmentVariable, InvalidPVCStorage, InvalidStatefulsetStorage, MountedFile, Storage,
};
use crate::cloud_provider::service::{get_service_statefulset_name_and_volumes, Action, Service, ServiceType};
use crate::deployment_action::DeploymentAction;
use crate::events::{EventDetails, Stage, Transmitter};
use crate::io_models::application::{ApplicationAdvancedSettings, Port};
use crate::io_models::context::Context;
use std::collections::BTreeSet;

use crate::cloud_provider::DeploymentTarget;
use crate::cloud_provider::Kind::Scw;
use crate::errors::EngineError;
use crate::io_models::application::Protocol::{TCP, UDP};
use crate::kubers_utils::kube_get_resources_by_selector;
use crate::models::container::{
    to_public_l4_ports, ClusterTeraContext, ContainerTeraContext, RegistryTeraContext, ServiceTeraContext,
};
use crate::models::probe::Probe;
use crate::models::types::{CloudProvider, ToTeraContext};
use crate::runtime::block_on;
use crate::unit_conversion::extract_volume_size;
use crate::utilities::to_short_id;
use itertools::Itertools;
use k8s_openapi::api::core::v1::PersistentVolumeClaim;
use std::marker::PhantomData;
use std::time::Duration;
use uuid::Uuid;

#[derive(thiserror::Error, Debug)]
pub enum ApplicationError {
    #[error("Application invalid configuration: {0}")]
    InvalidConfig(String),
}

pub struct Application<T: CloudProvider> {
    _marker: PhantomData<T>,
    pub(super) mk_event_details: Box<dyn Fn(Stage) -> EventDetails + Send + Sync>,
    pub(super) id: String,
    pub(super) long_id: Uuid,
    pub(super) action: Action,
    pub(super) name: String,
    pub(super) kube_name: String,
    pub(super) public_domain: String,
    pub(super) ports: Vec<Port>,
    pub(super) total_cpus: String,
    pub(super) cpu_burst: String,
    pub(super) total_ram_in_mib: u32,
    pub(super) min_instances: u32,
    pub(super) max_instances: u32,
    pub(super) build: Build,
    pub(super) command_args: Vec<String>,
    pub(super) entrypoint: Option<String>,
    pub(super) storage: Vec<Storage<T::StorageTypes>>,
    pub(super) environment_variables: Vec<EnvironmentVariable>,
    pub(super) mounted_files: BTreeSet<MountedFile>,
    pub(super) readiness_probe: Option<Probe>,
    pub(super) liveness_probe: Option<Probe>,
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
        kube_name: String,
        public_domain: String,
        ports: Vec<Port>,
        total_cpus: String,
        cpu_burst: String,
        total_ram_in_mib: u32,
        min_instances: u32,
        max_instances: u32,
        build: Build,
        command_args: Vec<String>,
        entrypoint: Option<String>,
        storage: Vec<Storage<T::StorageTypes>>,
        environment_variables: Vec<EnvironmentVariable>,
        mounted_files: BTreeSet<MountedFile>,
        readiness_probe: Option<Probe>,
        liveness_probe: Option<Probe>,
        advanced_settings: ApplicationAdvancedSettings,
        extra_settings: T::AppExtraSettings,
        mk_event_details: impl Fn(Transmitter) -> EventDetails,
    ) -> Result<Self, ApplicationError> {
        // TODO: Check that the information provided are coherent

        let workspace_directory = crate::fs::workspace_directory(
            context.workspace_root_dir(),
            context.execution_id(),
            format!("applications/{long_id}"),
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
            kube_name,
            public_domain,
            ports,
            total_cpus,
            cpu_burst,
            total_ram_in_mib,
            min_instances,
            max_instances,
            build,
            command_args,
            entrypoint,
            storage,
            environment_variables,
            mounted_files,
            readiness_probe,
            liveness_probe,
            advanced_settings,
            _extra_settings: extra_settings,
            workspace_directory,
            lib_root_directory: context.lib_root_dir().to_string(),
        })
    }

    pub fn helm_release_name(&self) -> String {
        crate::string::cut(format!("application-{}-{}", self.id(), self.id()), 50)
    }

    pub fn helm_chart_dir(&self) -> String {
        format!("{}/common/charts/q-container", self.lib_root_directory)
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
                r#type: "application",
                name: self.kube_name().to_string(),
                user_unsafe_name: self.name.clone(),
                image_full: self.build.image.full_image_name_with_tag(),
                image_tag: self.build.image.tag.clone(),
                version: self.version(),
                command_args: self.command_args.clone(),
                entrypoint: self.entrypoint.clone(),
                cpu_request_in_mili: self.total_cpus.clone(),
                cpu_limit_in_mili: self.total_cpus.clone(),
                ram_request_in_mib: format!("{}Mi", self.total_ram_in_mib),
                ram_limit_in_mib: format!("{}Mi", self.total_ram_in_mib),
                min_instances: self.min_instances,
                max_instances: self.max_instances,
                public_domain: self.public_domain.clone(),
                ports: self.ports.clone(),
                ports_layer4_public: {
                    let mut vec = Vec::with_capacity(2);
                    if let Some(tcp) = to_public_l4_ports(self.ports.iter(), TCP, &self.public_domain) {
                        vec.push(tcp);
                    }
                    if let Some(udp) = to_public_l4_ports(self.ports.iter(), UDP, &self.public_domain) {
                        vec.push(udp);
                    }
                    vec
                },
                default_port: self.ports.iter().find_or_first(|p| p.is_default).cloned(),
                storages: vec![],
                readiness_probe: self.readiness_probe.clone(),
                liveness_probe: self.liveness_probe.clone(),
                advanced_settings: self.advanced_settings.to_container_advanced_settings(),
                legacy_deployment_matchlabels: true,
                legacy_volumeclaim_template: true,
                legacy_deployment_from_scaleway: T::cloud_provider() == Scw,
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
            loadbalancer_l4_annotations: T::loadbalancer_l4_annotations(),
        };

        ctx
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

    pub fn kube_label_selector(&self) -> String {
        format!("qovery.com/service-id={}", self.long_id)
    }

    pub fn build(&self) -> &Build {
        &self.build
    }

    pub fn build_mut(&mut self) -> &mut Build {
        &mut self.build
    }

    pub fn command_args(&self) -> Vec<String> {
        self.command_args.clone()
    }

    pub fn entrypoint(&self) -> Option<String> {
        self.entrypoint.clone()
    }

    pub fn workspace_directory(&self) -> &str {
        &self.workspace_directory
    }

    fn service_version(&self) -> String {
        self.build.git_repository.commit_id.clone()
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

    fn get_environment_variables(&self) -> Vec<EnvironmentVariable> {
        self.environment_variables.clone()
    }
}

pub trait ApplicationService: Service + DeploymentAction + ToTeraContext + Send {
    fn get_build(&self) -> &Build;
    fn get_build_mut(&mut self) -> &mut Build;
    fn public_ports(&self) -> Vec<&Port>;
    fn advanced_settings(&self) -> &ApplicationAdvancedSettings;
    fn startup_timeout(&self) -> Duration;
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
}

pub fn get_application_with_invalid_storage_size<T: CloudProvider>(
    application: &Application<T>,
    kube_client: &kube::Client,
    namespace: &str,
    event_details: &EventDetails,
) -> Result<Option<InvalidStatefulsetStorage>, Box<EngineError>> {
    match !application.is_stateful() {
        true => Ok(None),
        false => {
            let selector = Application::kube_label_selector(application);
            let (statefulset_name, statefulset_volumes) =
                get_service_statefulset_name_and_volumes(kube_client, namespace, &selector, event_details)?;
            let storage_err = Box::new(EngineError::new_service_missing_storage(
                event_details.clone(),
                &application.long_id,
            ));
            let volumes = match statefulset_volumes {
                None => return Err(storage_err),
                Some(volumes) => volumes,
            };
            let mut invalid_storage = InvalidStatefulsetStorage {
                service_type: Application::service_type(application),
                service_id: application.long_id,
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

                            if let Some(storage) = application.storage.iter().find(|storage| volume_name == &storage.id)
                            {
                                if storage.size_in_gib > size {
                                    // if volume size in request is bigger than effective size we get related PVC to get its infos
                                    if let Some(pvc) =
                                        block_on(kube_get_resources_by_selector::<PersistentVolumeClaim>(
                                            kube_client,
                                            namespace,
                                            &format!("diskId={}", storage.id),
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
                                        None,
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

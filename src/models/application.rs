use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;
use std::path::PathBuf;
use std::time::Duration;

use itertools::Itertools;
use k8s_openapi::api::core::v1::PersistentVolumeClaim;
use uuid::Uuid;

use crate::build_platform::Build;
use crate::cloud_provider::models::{
    EnvironmentVariable, InvalidPVCStorage, InvalidStatefulsetStorage, KubernetesCpuResourceUnit,
    KubernetesMemoryResourceUnit, MountedFile, Storage, StorageDataTemplate,
};
use crate::cloud_provider::service::{get_service_statefulset_name_and_volumes, Action, Service, ServiceType};
use crate::cloud_provider::DeploymentTarget;
use crate::cloud_provider::Kind::Scw;
use crate::deployment_action::DeploymentAction;
use crate::errors::EngineError;
use crate::events::{EventDetails, Stage, Transmitter};
use crate::io_models::annotations_group::AnnotationsGroup;
use crate::io_models::application::Protocol::{TCP, UDP};
use crate::io_models::application::{ApplicationAdvancedSettings, Port};
use crate::io_models::context::Context;
use crate::io_models::labels_group::LabelsGroup;
use crate::kubers_utils::kube_get_resources_by_selector;
use crate::models::annotations_group::AnnotationsGroupTeraContext;
use crate::models::container::{
    to_public_l4_ports, ClusterTeraContext, ContainerTeraContext, RegistryTeraContext, ServiceTeraContext,
};
use crate::models::labels_group::LabelsGroupTeraContext;
use crate::models::probe::Probe;
use crate::models::types::{CloudProvider, ToTeraContext};
use crate::models::utils;
use crate::runtime::block_on;
use crate::unit_conversion::extract_volume_size;
use crate::utilities::to_short_id;

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
    pub(super) cpu_request_in_milli: KubernetesCpuResourceUnit,
    pub(super) cpu_limit_in_milli: KubernetesCpuResourceUnit,
    pub(super) ram_request_in_mib: KubernetesMemoryResourceUnit,
    pub(super) ram_limit_in_mib: KubernetesMemoryResourceUnit,
    pub(super) min_instances: u32,
    pub(super) max_instances: u32,
    pub(super) build: Build,
    pub(super) command_args: Vec<String>,
    pub(super) entrypoint: Option<String>,
    pub(super) storages: Vec<Storage>,
    pub(super) environment_variables: Vec<EnvironmentVariable>,
    pub(super) mounted_files: BTreeSet<MountedFile>,
    pub(super) readiness_probe: Option<Probe>,
    pub(super) liveness_probe: Option<Probe>,
    pub(super) advanced_settings: ApplicationAdvancedSettings,
    pub(super) _extra_settings: T::AppExtraSettings,
    pub(super) workspace_directory: PathBuf,
    pub(super) lib_root_directory: String,
    pub(super) annotations_group: AnnotationsGroupTeraContext,
    pub(super) labels_group: LabelsGroupTeraContext,
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
        min_instances: u32,
        max_instances: u32,
        build: Build,
        command_args: Vec<String>,
        entrypoint: Option<String>,
        storages: Vec<Storage>,
        environment_variables: Vec<EnvironmentVariable>,
        mounted_files: BTreeSet<MountedFile>,
        readiness_probe: Option<Probe>,
        liveness_probe: Option<Probe>,
        advanced_settings: ApplicationAdvancedSettings,
        extra_settings: T::AppExtraSettings,
        mk_event_details: impl Fn(Transmitter) -> EventDetails,
        annotations_groups: Vec<AnnotationsGroup>,
        labels_groups: Vec<LabelsGroup>,
        cpu_request_in_milli: KubernetesCpuResourceUnit,
        cpu_limit_in_milli: KubernetesCpuResourceUnit,
        ram_request_in_mib: KubernetesMemoryResourceUnit,
        ram_limit_in_mib: KubernetesMemoryResourceUnit,
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
            cpu_request_in_milli,
            cpu_limit_in_milli,
            ram_request_in_mib,
            ram_limit_in_mib,
            min_instances,
            max_instances,
            build,
            command_args,
            entrypoint,
            storages,
            environment_variables,
            mounted_files,
            readiness_probe,
            liveness_probe,
            advanced_settings,
            _extra_settings: extra_settings,
            workspace_directory,
            lib_root_directory: context.lib_root_dir().to_string(),
            annotations_group: AnnotationsGroupTeraContext::new(annotations_groups),
            labels_group: LabelsGroupTeraContext::new(labels_groups),
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
        let environment = target.environment;
        let kubernetes = target.kubernetes;
        let mut deployment_affinity_node_required = utils::add_arch_to_deployment_affinity_node(
            &self.advanced_settings.deployment_affinity_node_required,
            &target.kubernetes.cpu_architectures(),
        );

        let mut tolerations = BTreeMap::<String, String>::new();
        let is_stateful_set = !self.storages.is_empty();
        if utils::need_target_stable_node_pool(kubernetes, self.min_instances, is_stateful_set) {
            utils::target_stable_node_pool(&mut deployment_affinity_node_required, &mut tolerations, is_stateful_set);
        }

        let mut advanced_settings = self.advanced_settings.clone();
        advanced_settings.deployment_affinity_node_required = deployment_affinity_node_required;
        let registry_info = target.container_registry.registry_info();
        let ctx = ContainerTeraContext {
            organization_long_id: environment.organization_long_id,
            project_long_id: environment.project_long_id,
            environment_short_id: to_short_id(&environment.long_id),
            environment_long_id: environment.long_id,
            cluster: ClusterTeraContext::from(kubernetes),
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
                cpu_request_in_milli: self.cpu_request_in_milli.to_string(),
                cpu_limit_in_milli: self.cpu_limit_in_milli.to_string(),
                ram_request_in_mib: self.ram_request_in_mib.to_string(),
                ram_limit_in_mib: self.ram_limit_in_mib.to_string(),
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
                storages: self
                    .storages
                    .iter()
                    .map(|s| StorageDataTemplate {
                        id: s.id.clone(),
                        long_id: s.long_id,
                        name: s.name.clone(),
                        storage_type: s.storage_class.0.clone(),
                        size_in_gib: s.size_in_gib,
                        mount_point: s.mount_point.clone(),
                        snapshot_retention_in_days: s.snapshot_retention_in_days,
                    })
                    .collect(),
                readiness_probe: self.readiness_probe.clone(),
                liveness_probe: self.liveness_probe.clone(),
                advanced_settings: advanced_settings.to_container_advanced_settings(),
                legacy_deployment_matchlabels: true,
                legacy_volumeclaim_template: true,
                legacy_deployment_from_scaleway: T::cloud_provider() == Scw,
                tolerations,
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
            loadbalancer_l4_annotations: kubernetes.loadbalancer_l4_annotations(Some(self.kube_name())),
            annotations_group: self.annotations_group.clone(),
            labels_group: self.labels_group.clone(),
        };

        ctx
    }

    pub fn is_stateful(&self) -> bool {
        !self.storages.is_empty()
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

    pub fn kube_legacy_label_selector(&self) -> String {
        format!("appId={}", self.id)
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
        self.workspace_directory.to_str().unwrap_or("")
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

use tera::Context as TeraContext;
impl<T: CloudProvider> ToTeraContext for Application<T> {
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, Box<EngineError>> {
        let context = self.default_tera_context(target);
        Ok(TeraContext::from_serialize(context).unwrap_or_default())
    }
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

                            if let Some(storage) =
                                application.storages.iter().find(|storage| volume_name == &storage.id)
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

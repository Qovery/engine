use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;
use std::path::PathBuf;
use std::time::Duration;

use itertools::Itertools;
use k8s_openapi::api::core::v1::PersistentVolumeClaim;
use serde::Serialize;
use uuid::Uuid;

use crate::environment::action::DeploymentAction;
use crate::environment::models::annotations_group::AnnotationsGroupTeraContext;
use crate::environment::models::labels_group::LabelsGroupTeraContext;
use crate::environment::models::probe::Probe;
use crate::environment::models::registry_image_source::RegistryImageSource;
use crate::environment::models::types::{CloudProvider, ToTeraContext};
use crate::environment::models::utils;
use crate::errors::EngineError;
use crate::events::{EventDetails, Stage, Transmitter};
use crate::infrastructure::models::build_platform::Build;
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::infrastructure::models::cloud_provider::io::RegistryMirroringMode;
use crate::infrastructure::models::cloud_provider::service::{
    Action, Service, ServiceType, get_service_statefulset_name_and_volumes,
};
use crate::infrastructure::models::container_registry::DockerRegistryInfo;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::io_models::annotations_group::AnnotationsGroup;
use crate::io_models::application::Protocol::{TCP, UDP};
use crate::io_models::application::{Port, Protocol};
use crate::io_models::container::{ContainerAdvancedSettings, Registry};
use crate::io_models::context::Context;
use crate::io_models::labels_group::LabelsGroup;
use crate::io_models::models::{
    EnvironmentVariable, InvalidPVCStorage, InvalidStatefulsetStorage, KubernetesCpuResourceUnit,
    KubernetesMemoryResourceUnit, MountedFile, Storage, StorageDataTemplate,
};
use crate::kubers_utils::kube_get_resources_by_selector;
use crate::runtime::block_on;
use crate::unit_conversion::extract_volume_size;
use crate::utilities::to_short_id;

#[derive(thiserror::Error, Debug)]
pub enum ContainerError {
    #[error("Container invalid configuration: {0}")]
    InvalidConfig(String),
}

pub struct Container<T: CloudProvider> {
    _marker: PhantomData<T>,
    pub(crate) mk_event_details: Box<dyn Fn(Stage) -> EventDetails + Send + Sync>,
    pub(crate) id: String,
    pub(crate) long_id: Uuid,
    pub(crate) name: String,
    pub(crate) kube_name: String,
    pub(crate) action: Action,
    pub source: RegistryImageSource,
    pub(crate) command_args: Vec<String>,
    pub(crate) entrypoint: Option<String>,
    pub(crate) cpu_request_in_milli: KubernetesCpuResourceUnit,
    pub(crate) cpu_limit_in_milli: KubernetesCpuResourceUnit,
    pub(crate) ram_request_in_mib: KubernetesMemoryResourceUnit,
    pub(crate) ram_limit_in_mib: KubernetesMemoryResourceUnit,
    pub(crate) min_instances: u32,
    pub(crate) max_instances: u32,
    pub(crate) public_domain: String,
    pub(crate) ports: Vec<Port>,
    pub(crate) storages: Vec<Storage>,
    pub(crate) environment_variables: Vec<EnvironmentVariable>,
    pub(crate) mounted_files: BTreeSet<MountedFile>,
    pub(crate) readiness_probe: Option<Probe>,
    pub(crate) liveness_probe: Option<Probe>,
    pub(crate) advanced_settings: ContainerAdvancedSettings,
    pub(crate) _extra_settings: T::AppExtraSettings,
    pub(crate) workspace_directory: PathBuf,
    pub(crate) lib_root_directory: String,
    pub(crate) annotations_group: AnnotationsGroupTeraContext,
    pub(crate) labels_group: LabelsGroupTeraContext,
}

pub fn get_mirror_repository_name(
    service_id: &Uuid,
    cluster_id: &Uuid,
    registry_mirroring_mode: &RegistryMirroringMode,
) -> String {
    match registry_mirroring_mode {
        RegistryMirroringMode::Cluster => format!("qovery-mirror-cluster-{cluster_id}"),
        RegistryMirroringMode::Service => format!("qovery-mirror-{service_id}"),
    }
}

pub fn to_public_l4_ports<'a>(
    ports: impl Iterator<Item = &'a Port>,
    protocol: Protocol,
    public_domain: &str,
) -> Option<PublicL4Ports> {
    let ports: Vec<Port> = ports
        .filter(|p| p.publicly_accessible && p.protocol == protocol)
        .cloned()
        .collect();
    if ports.is_empty() {
        None
    } else {
        Some(PublicL4Ports {
            protocol,
            hostnames: ports.iter().map(|p| format!("{}-{}", p.name, public_domain)).collect(),
            ports,
        })
    }
}

// Here we define the common behavior among all providers
impl<T: CloudProvider> Container<T> {
    pub fn new(
        context: &Context,
        long_id: Uuid,
        name: String,
        kube_name: String,
        action: Action,
        registry_image_source: RegistryImageSource,
        command_args: Vec<String>,
        entrypoint: Option<String>,
        cpu_request_in_milli: KubernetesCpuResourceUnit,
        cpu_limit_in_milli: KubernetesCpuResourceUnit,
        ram_request_in_mib: KubernetesMemoryResourceUnit,
        ram_limit_in_mib: KubernetesMemoryResourceUnit,
        min_instances: u32,
        max_instances: u32,
        public_domain: String,
        ports: Vec<Port>,
        storages: Vec<Storage>,
        environment_variables: Vec<EnvironmentVariable>,
        mounted_files: BTreeSet<MountedFile>,
        readiness_probe: Option<Probe>,
        liveness_probe: Option<Probe>,
        advanced_settings: ContainerAdvancedSettings,
        extra_settings: T::AppExtraSettings,
        mk_event_details: impl Fn(Transmitter) -> EventDetails,
        annotations_groups: Vec<AnnotationsGroup>,
        labels_groups: Vec<LabelsGroup>,
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
            kube_name,
            source: registry_image_source,
            command_args,
            entrypoint,
            cpu_request_in_milli,
            cpu_limit_in_milli,
            ram_request_in_mib,
            ram_limit_in_mib,
            min_instances,
            max_instances,
            public_domain,
            ports,
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

    pub fn helm_selector(&self) -> Option<String> {
        Some(self.kube_label_selector())
    }

    pub fn helm_release_name(&self) -> String {
        format!("container-{}", self.long_id)
    }

    pub fn helm_chart_dir(&self) -> String {
        format!("{}/common/charts/q-container", self.lib_root_directory)
    }

    pub fn registry(&self) -> &Registry {
        &self.source.registry
    }

    fn public_ports(&self) -> impl Iterator<Item = &Port> + '_ {
        self.ports.iter().filter(|port| port.publicly_accessible)
    }

    pub(crate) fn default_tera_context(&self, target: &DeploymentTarget) -> ContainerTeraContext {
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
        let registry_endpoint = registry_info.get_registry_endpoint(Some(target.kubernetes.cluster_name().as_str()));
        let registry_endpoint_host = registry_endpoint.host_str().unwrap_or_default();
        let repository: Cow<str> = if let Some(port) = registry_endpoint.port() {
            format!("{registry_endpoint_host}:{port}").into()
        } else {
            registry_endpoint_host.into()
        };

        let (_, image_name, image_tag, _) = self
            .source
            .compute_cluster_container_registry_url_with_image_name_and_image_tag(
                self.long_id(),
                target.kubernetes.long_id(),
                &target.kubernetes.advanced_settings().registry_mirroring_mode,
                target.container_registry.registry_info(),
            );
        let image_full = format!("{repository}/{image_name}:{image_tag}");

        ContainerTeraContext {
            organization_long_id: environment.organization_long_id,
            project_long_id: environment.project_long_id,
            environment_short_id: to_short_id(&environment.long_id),
            environment_long_id: environment.long_id,
            cluster: ClusterTeraContext::from(kubernetes),
            namespace: environment.namespace().to_string(),
            service: ServiceTeraContext {
                short_id: to_short_id(&self.long_id),
                long_id: self.long_id,
                r#type: "container",
                name: self.kube_name().to_string(),
                user_unsafe_name: self.name.clone(),
                // FIXME: We mirror images to cluster private registry
                image_full,
                image_tag,
                version: self.service_version(),
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
                advanced_settings,
                legacy_deployment_matchlabels: false,
                legacy_volumeclaim_template: false,
                legacy_deployment_from_scaleway: false,
                tolerations,
            },
            registry: registry_info
                .get_registry_docker_json_config(DockerRegistryInfo {
                    registry_name: Some(kubernetes.cluster_name()), // TODO(benjaminch): this is a bit of a hack, considering registry name will be the same as cluster one, it should be the case, but worth doing it better
                    repository_name: None,
                    image_name: Some(self.source.image.to_string()),
                })
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
        }
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

    pub fn kube_label_selector(&self) -> String {
        format!("qovery.com/service-id={}", self.long_id)
    }

    pub fn kube_legacy_label_selector(&self) -> String {
        format!("appId={}", self.id)
    }

    pub fn workspace_directory(&self) -> &str {
        self.workspace_directory.to_str().unwrap_or("")
    }

    fn service_version(&self) -> String {
        format!("{}:{}", self.source.image, self.source.tag)
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
        None
    }

    fn build_mut(&mut self) -> Option<&mut Build> {
        None
    }

    fn get_environment_variables(&self) -> Vec<EnvironmentVariable> {
        self.environment_variables.clone()
    }
}

pub trait ContainerService: Service + DeploymentAction + ToTeraContext + Send {
    fn public_ports(&self) -> Vec<&Port>;
    fn advanced_settings(&self) -> &ContainerAdvancedSettings;
    fn image_full(&self) -> String;
    fn startup_timeout(&self) -> Duration;
    fn as_deployment_action(&self) -> &dyn DeploymentAction;
}

use tera::Context as TeraContext;

impl<T: CloudProvider> ToTeraContext for Container<T> {
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, Box<EngineError>> {
        let context = self.default_tera_context(target);
        Ok(TeraContext::from_serialize(context).unwrap_or_default())
    }
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
            self.source.registry.url().to_string().trim_start_matches("https://"),
            self.source.image,
            self.source.tag
        )
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
        let startup_timeout = std::cmp::max(probe_timeout /* * 10 rolling restart percent */, 60 * 20);
        Duration::from_secs(startup_timeout as u64)
    }

    fn as_deployment_action(&self) -> &dyn DeploymentAction {
        self
    }
}

#[derive(Serialize, Debug, Clone)]
pub(crate) struct ClusterTeraContext {
    pub(crate) long_id: Uuid,
    pub(crate) name: String,
    pub(crate) region: String,
    pub(crate) zone: String,
}

impl From<&dyn Kubernetes> for ClusterTeraContext {
    fn from(k: &dyn Kubernetes) -> Self {
        Self {
            long_id: *k.long_id(),
            name: k.name().to_string(),
            region: k.region().to_string(),
            zone: k.default_zone().unwrap_or("").to_string(),
        }
    }
}

#[derive(Serialize, Debug, Clone)]
pub struct PublicL4Ports {
    pub protocol: Protocol,
    pub ports: Vec<Port>,
    pub hostnames: Vec<String>,
}

#[derive(Serialize, Debug, Clone)]
pub(crate) struct ServiceTeraContext {
    pub(crate) short_id: String,
    pub(crate) long_id: Uuid,
    pub(crate) r#type: &'static str,
    pub(crate) name: String,
    pub(crate) user_unsafe_name: String,
    pub(crate) image_full: String,
    pub(crate) image_tag: String,
    pub(crate) version: String,
    pub(crate) command_args: Vec<String>,
    pub(crate) entrypoint: Option<String>,
    pub(crate) cpu_request_in_milli: String,
    pub(crate) cpu_limit_in_milli: String,
    pub(crate) ram_request_in_mib: String,
    pub(crate) ram_limit_in_mib: String,
    pub(crate) min_instances: u32,
    pub(crate) max_instances: u32,
    pub(crate) public_domain: String,
    pub(crate) ports: Vec<Port>,
    pub(crate) ports_layer4_public: Vec<PublicL4Ports>,
    pub(crate) default_port: Option<Port>,
    pub(crate) storages: Vec<StorageDataTemplate>,
    pub(crate) readiness_probe: Option<Probe>,
    pub(crate) liveness_probe: Option<Probe>,
    pub(crate) advanced_settings: ContainerAdvancedSettings,
    pub(crate) legacy_deployment_matchlabels: bool,
    pub(crate) legacy_volumeclaim_template: bool,
    pub(crate) legacy_deployment_from_scaleway: bool,
    pub(crate) tolerations: BTreeMap<String, String>,
}

#[derive(Serialize, Debug, Clone)]
pub(crate) struct RegistryTeraContext {
    pub(crate) secret_name: String,
    pub(crate) docker_json_config: Option<String>,
}

#[derive(Serialize, Debug, Clone)]
pub(crate) struct ContainerTeraContext {
    pub(crate) organization_long_id: Uuid,
    pub(crate) project_long_id: Uuid,
    pub(crate) environment_short_id: String,
    pub(crate) environment_long_id: Uuid,
    pub(crate) cluster: ClusterTeraContext,
    pub(crate) namespace: String,
    pub(crate) service: ServiceTeraContext,
    pub(crate) registry: Option<RegistryTeraContext>,
    pub(crate) environment_variables: Vec<EnvironmentVariable>,
    pub(crate) mounted_files: Vec<MountedFile>,
    pub(crate) resource_expiration_in_seconds: Option<i32>,
    pub(crate) loadbalancer_l4_annotations: Vec<(String, String)>,
    pub(crate) annotations_group: AnnotationsGroupTeraContext,
    pub(crate) labels_group: LabelsGroupTeraContext,
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
            let selector = Container::kube_label_selector(container);
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

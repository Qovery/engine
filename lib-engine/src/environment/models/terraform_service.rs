use crate::environment::action::DeploymentAction;
use crate::environment::models::annotations_group::AnnotationsGroupTeraContext;
use crate::environment::models::labels_group::LabelsGroupTeraContext;
use crate::environment::models::types::{CloudProvider, VersionsNumber};
use crate::environment::models::utils;
use crate::events::{EventDetails, Stage, Transmitter};
use crate::infrastructure::models::build_platform::{Build, Credentials, SshKey};
use crate::infrastructure::models::cloud_provider::service::{Action, Service, ServiceType};
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::io_models::annotations_group::AnnotationsGroup;
use crate::io_models::context::Context;
use crate::io_models::labels_group::LabelsGroup;
use crate::io_models::models::{EnvironmentVariable, KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};
use crate::io_models::terraform_service::TerraformServiceAdvancedSettings;
use crate::io_models::variable_utils::VariableInfo;
use crate::utilities::to_short_id;
use serde_derive::Serialize;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::time::Duration;
use url::Url;
use uuid::Uuid;

#[derive(thiserror::Error, Debug)]
pub enum TerraformServiceError {
    #[error("Terraform Service invalid configuration: {0}")]
    InvalidConfig(String),
}

pub struct TerraformService<T: CloudProvider> {
    _marker: PhantomData<T>,
    pub(crate) mk_event_details: Box<dyn Fn(Stage) -> EventDetails + Send + Sync>,
    pub(crate) id: String,
    pub(crate) long_id: Uuid,
    pub(crate) name: String,
    pub(crate) kube_name: String,
    pub(crate) action: Action,
    pub(crate) build: Build,
    pub(crate) terraform_files_source: TerraformFilesSource,
    pub(crate) _provider: TerraformProvider,
    pub(crate) _provider_version: VersionsNumber,
    pub(crate) _backend: TerraformBackend,
    pub(crate) terraform_action: TerraformAction,
    pub(crate) timeout: Duration,
    pub(crate) cpu_request: KubernetesCpuResourceUnit,
    pub(crate) cpu_limit: KubernetesCpuResourceUnit,
    pub(crate) ram_request: KubernetesMemoryResourceUnit,
    pub(crate) ram_limit: KubernetesMemoryResourceUnit,
    pub(crate) environment_variables: HashMap<String, VariableInfo>,
    pub(crate) advanced_settings: TerraformServiceAdvancedSettings,
    pub(crate) annotations_group: AnnotationsGroupTeraContext,
    pub(crate) labels_group: LabelsGroupTeraContext,
    pub(crate) workspace_directory: PathBuf,
    pub(crate) lib_root_directory: String,
}

impl<T: CloudProvider> TerraformService<T> {
    pub fn new(
        context: &Context,
        long_id: Uuid,
        name: String,
        kube_name: String,
        action: Action,
        build: Build,
        terraform_files_source: TerraformFilesSource,
        _provider: TerraformProvider,
        _provider_version: VersionsNumber,
        _backend: TerraformBackend,
        terraform_action: TerraformAction,
        timeout: Duration,
        environment_variables: HashMap<String, VariableInfo>,
        advanced_settings: TerraformServiceAdvancedSettings,
        mk_event_details: impl Fn(Transmitter) -> EventDetails,
        annotations_groups: Vec<AnnotationsGroup>,
        labels_groups: Vec<LabelsGroup>,
    ) -> Result<Self, TerraformServiceError> {
        let event_details = mk_event_details(Transmitter::TerraformService(long_id, name.to_string()));
        let mk_event_details = move |stage: Stage| EventDetails::clone_changing_stage(event_details.clone(), stage);

        let workspace_directory = crate::fs::workspace_directory(
            context.workspace_root_dir(),
            context.execution_id(),
            format!("terraform_services/{long_id}"),
        )
        .map_err(|_| TerraformServiceError::InvalidConfig("Can't create workspace directory".to_string()))?;

        Ok(Self {
            _marker: PhantomData,
            mk_event_details: Box::new(mk_event_details),
            id: to_short_id(&long_id),
            long_id,
            name,
            kube_name,
            action,
            build,
            terraform_files_source,
            _provider,
            _provider_version,
            _backend,
            terraform_action,
            timeout,
            cpu_request: KubernetesCpuResourceUnit::MilliCpu(500), // TODO TF set in service parameter or advanced settings
            cpu_limit: KubernetesCpuResourceUnit::MilliCpu(500), // TODO TF set in service parameter or advanced settings
            ram_request: KubernetesMemoryResourceUnit::MebiByte(256), // TODO TF set in service parameter or advanced settings
            ram_limit: KubernetesMemoryResourceUnit::MebiByte(256), // TODO TF set in service parameter or advanced settings
            environment_variables,
            advanced_settings,
            annotations_group: AnnotationsGroupTeraContext::new(annotations_groups),
            labels_group: LabelsGroupTeraContext::new(labels_groups),
            workspace_directory,
            lib_root_directory: context.lib_root_dir().to_string(),
        })
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

    pub fn service_version(&self) -> String {
        match &self.terraform_files_source {
            TerraformFilesSource::Git { commit_id, .. } => commit_id.to_string(),
        }
    }
    pub fn service_type(&self) -> ServiceType {
        ServiceType::Terraform
    }

    pub fn kube_label_selector(&self) -> String {
        format!("qovery.com/service-id={}", self.long_id)
    }

    pub fn workspace_directory(&self) -> &str {
        self.workspace_directory.to_str().unwrap_or("")
    }

    pub fn helm_release_name(&self) -> String {
        format!("tf-service-{}", self.long_id)
    }

    pub fn startup_timeout(&self) -> Duration {
        Duration::from_secs(5 * 60)
    }

    pub fn helm_chart_dir(&self) -> String {
        format!("{}/common/charts/q-terraform-service", self.lib_root_directory)
    }

    pub(crate) fn default_tera_context(&self, target: &DeploymentTarget) -> TerraformServiceTeraContext {
        let environment = target.environment;
        let (image_full, image_tag) = match &self.terraform_files_source {
            TerraformFilesSource::Git { .. } => {
                (self.build.image.full_image_name_with_tag(), self.build.image.tag.clone())
            }
        };

        let deployment_affinity_node_required = utils::add_arch_to_deployment_affinity_node(
            &self.advanced_settings.deployment_affinity_node_required,
            &target.kubernetes.cpu_architectures(),
        );
        let mut advanced_settings = self.advanced_settings.clone();
        advanced_settings.deployment_affinity_node_required = deployment_affinity_node_required;

        TerraformServiceTeraContext {
            organization_long_id: environment.organization_long_id,
            project_long_id: environment.project_long_id,
            environment_short_id: to_short_id(&environment.long_id),
            environment_long_id: environment.long_id,
            namespace: environment.namespace().to_string(),
            service: ServiceTeraContext {
                short_id: to_short_id(&self.long_id),
                long_id: self.long_id,
                name: self.kube_name().to_string(),
                image_full,
                image_tag,
                version: self.service_version(),
                job_max_duration_in_sec: self.timeout.as_secs(),
                advanced_settings,
                entrypoint: "entrypoint.sh".to_string(),
                command_args: match &self.terraform_action {
                    TerraformAction::TerraformPlanOnly => vec!["plan_only".to_string()],
                    TerraformAction::TerraformPlanAndApply => vec!["apply".to_string()],
                    TerraformAction::TerraformApplyFromPlan(execution_id) => {
                        vec!["apply_from_plan".to_string(), execution_id.to_owned()]
                    }
                },
                // entrypoint: self.entrypoint.clone(),
                cpu_request_in_milli: self.cpu_request.to_string(), // TODO TF check if it is provided as advanced setting or not
                cpu_limit_in_milli: self.cpu_limit.to_string(),     // TODO TF
                ram_request_in_mib: self.ram_request.to_string(),   // TODO TF
                ram_limit_in_mib: self.ram_limit.to_string(),       // TODO TF
                // max_nb_restart: self.max_nb_restart,
                // max_duration_in_sec: self.max_duration.as_secs(),
                persistence_size_in_gib: "1".to_string(),    // TODO TF
                persistence_storage_type: "gp2".to_string(), // TODO TF
            },
            annotations_group: self.annotations_group.clone(),
            labels_group: self.labels_group.clone(),
            environment_variables: self.get_environment_variables(),
        }
    }
}

impl<T: CloudProvider> Service for TerraformService<T> {
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
        Some(&self.build)
    }

    fn build_mut(&mut self) -> Option<&mut Build> {
        Some(&mut self.build)
    }

    fn get_environment_variables(&self) -> Vec<EnvironmentVariable> {
        self.environment_variables
            .iter()
            .map(|(key, variable_infos)| EnvironmentVariable {
                key: key.clone(),
                value: variable_infos.value.clone(),
                is_secret: variable_infos.is_secret,
            })
            .collect()
    }
}

pub trait TerraformServiceTrait: Service + DeploymentAction + Send {
    fn advanced_settings(&self) -> &TerraformServiceAdvancedSettings;
    fn as_deployment_action(&self) -> &dyn DeploymentAction;
    fn job_max_duration(&self) -> &Duration;
}

impl<T: CloudProvider> TerraformServiceTrait for TerraformService<T>
where
    TerraformService<T>: Service + DeploymentAction,
{
    fn advanced_settings(&self) -> &TerraformServiceAdvancedSettings {
        &self.advanced_settings
    }
    fn as_deployment_action(&self) -> &dyn DeploymentAction {
        self
    }

    fn job_max_duration(&self) -> &Duration {
        &self.timeout
    }
}

pub enum TerraformFilesSource {
    Git {
        git_url: Url,
        get_credentials: Box<dyn Fn() -> anyhow::Result<Option<Credentials>> + Send + Sync>,
        commit_id: String,
        root_module_path: String,
        ssh_keys: Vec<SshKey>,
    },
}

pub enum TerraformProvider {
    Terraform,
    // OpenTofu
}

pub enum TerraformAction {
    TerraformPlanOnly,
    TerraformPlanAndApply,
    TerraformApplyFromPlan(String),
}

#[allow(dead_code)]
pub struct TerraformBackendBlock(String);

impl From<&String> for TerraformBackendBlock {
    fn from(value: &String) -> Self {
        TerraformBackendBlock(value.to_string())
    }
}

pub struct TerraformBackend {
    pub backend_type: TerraformBackendType,
    pub block: TerraformBackendBlock,
    pub configs: Vec<TerraformBackendConfig>,
}

pub enum TerraformBackendType {
    DefinedInTerraformFile,
    Kubernetes,
}

pub struct TerraformBackendConfig {
    pub key: String,
    pub value: String,
}

#[derive(Serialize, Debug, Clone)]
pub(crate) struct ServiceTeraContext {
    pub(crate) short_id: String,
    pub(crate) long_id: Uuid,
    pub(crate) name: String,
    pub(crate) image_full: String,
    pub(crate) image_tag: String,
    pub(crate) version: String,
    pub(crate) job_max_duration_in_sec: u64,
    pub(crate) cpu_request_in_milli: String,
    pub(crate) cpu_limit_in_milli: String,
    pub(crate) ram_request_in_mib: String,
    pub(crate) ram_limit_in_mib: String,
    pub(crate) advanced_settings: TerraformServiceAdvancedSettings,
    pub(crate) entrypoint: String,
    pub(crate) command_args: Vec<String>,
    pub(crate) persistence_size_in_gib: String,
    pub(crate) persistence_storage_type: String,
}

#[derive(Serialize, Debug, Clone)]
pub(crate) struct TerraformServiceTeraContext {
    pub(crate) organization_long_id: Uuid,
    pub(crate) project_long_id: Uuid,
    pub(crate) environment_short_id: String,
    pub(crate) environment_long_id: Uuid,
    pub(crate) namespace: String,
    pub(crate) service: ServiceTeraContext,
    pub(crate) annotations_group: AnnotationsGroupTeraContext,
    pub(crate) labels_group: LabelsGroupTeraContext,
    pub(crate) environment_variables: Vec<EnvironmentVariable>,
}

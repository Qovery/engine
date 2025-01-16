use crate::environment::action::DeploymentAction;
use crate::environment::models::annotations_group::AnnotationsGroupTeraContext;
use crate::environment::models::labels_group::LabelsGroupTeraContext;
use crate::environment::models::types::{CloudProvider, VersionsNumber};
use crate::events::{EventDetails, Stage, Transmitter};
use crate::infrastructure::models::build_platform::{Build, Credentials, SshKey};
use crate::infrastructure::models::cloud_provider::service::{Action, Service, ServiceType};
use crate::io_models::annotations_group::AnnotationsGroup;
use crate::io_models::context::Context;
use crate::io_models::labels_group::LabelsGroup;
use crate::io_models::models::EnvironmentVariable;
use crate::io_models::terraform_service::TerraformServiceAdvancedSettings;
use crate::io_models::variable_utils::VariableInfo;
use crate::utilities::to_short_id;
use std::collections::HashMap;
use std::marker::PhantomData;
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
    pub(crate) environment_variables: HashMap<String, VariableInfo>,
    pub(crate) advanced_settings: TerraformServiceAdvancedSettings,
    pub(crate) _annotations_group: AnnotationsGroupTeraContext,
    pub(crate) _labels_group: LabelsGroupTeraContext,
}

impl<T: CloudProvider> TerraformService<T> {
    pub fn new(
        _context: &Context,
        long_id: Uuid,
        name: String,
        kube_name: String,
        action: Action,
        build: Build,
        terraform_files_source: TerraformFilesSource,
        _provider: TerraformProvider,
        _provider_version: VersionsNumber,
        _backend: TerraformBackend,
        environment_variables: HashMap<String, VariableInfo>,
        advanced_settings: TerraformServiceAdvancedSettings,
        mk_event_details: impl Fn(Transmitter) -> EventDetails,
        annotations_groups: Vec<AnnotationsGroup>,
        labels_groups: Vec<LabelsGroup>,
    ) -> Result<Self, TerraformServiceError> {
        let event_details = mk_event_details(Transmitter::TerraformService(long_id, name.to_string()));
        let mk_event_details = move |stage: Stage| EventDetails::clone_changing_stage(event_details.clone(), stage);
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
            environment_variables,
            advanced_settings,
            _annotations_group: AnnotationsGroupTeraContext::new(annotations_groups),
            _labels_group: LabelsGroupTeraContext::new(labels_groups),
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

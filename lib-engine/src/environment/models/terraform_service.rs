use crate::environment::action::DeploymentAction;
use crate::environment::models::annotations_group::AnnotationsGroupTeraContext;
use crate::environment::models::container::RegistryTeraContext;
use crate::environment::models::labels_group::LabelsGroupTeraContext;
use crate::environment::models::types::CloudProvider;
use crate::environment::models::utils;
use crate::events::{EventDetails, Stage, Transmitter};
use crate::infrastructure::models::build_platform::{Build, Credentials, SshKey};
use crate::infrastructure::models::cloud_provider::service::{Action, Service, ServiceType};
use crate::infrastructure::models::cloud_provider::{DeploymentTarget, Kind};
use crate::infrastructure::models::container_registry::DockerRegistryInfo;
use crate::io_models::annotations_group::AnnotationsGroup;
use crate::io_models::context::Context;
use crate::io_models::labels_group::LabelsGroup;
use crate::io_models::models::{EnvironmentVariable, KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};
use crate::io_models::terraform_service::TerraformServiceAdvancedSettings;
use crate::io_models::variable_utils::VariableInfo;
use crate::utilities::to_short_id;
use base64::Engine;
use base64::engine::general_purpose;
use serde_derive::Serialize;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::str::FromStr;
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
    pub(crate) deployment_id: String,
    pub(crate) name: String,
    pub(crate) kube_name: String,
    pub(crate) action: Action,
    pub(crate) build: Build,
    pub(crate) root_module_path: PathBuf,
    pub(crate) terraform_files_source: TerraformFilesSource,
    pub(crate) terraform_var_file_paths: Vec<String>,
    pub(crate) terraform_vars: Vec<(String, String)>,
    pub(crate) backend: TerraformBackend,
    pub(crate) terraform_action: TerraformAction,
    pub(crate) timeout: Duration,
    pub(crate) cpu_request: KubernetesCpuResourceUnit,
    pub(crate) cpu_limit: KubernetesCpuResourceUnit,
    pub(crate) ram_request: KubernetesMemoryResourceUnit,
    pub(crate) ram_limit: KubernetesMemoryResourceUnit,
    pub(crate) persistent_storage: PersistentStorage,
    pub(crate) environment_variables: HashMap<String, VariableInfo>,
    pub(crate) advanced_settings: TerraformServiceAdvancedSettings,
    pub(crate) annotations_group: AnnotationsGroupTeraContext,
    pub(crate) labels_group: LabelsGroupTeraContext,
    pub(crate) workspace_directory: PathBuf,
    pub(crate) lib_root_directory: String,
    pub(crate) terraform_credentials: TerraformCredentials,
}

impl<T: CloudProvider> TerraformService<T> {
    pub fn new(
        context: &Context,
        long_id: Uuid,
        name: String,
        kube_name: String,
        action: Action,
        cpu_request_in_milli: u32,
        cpu_limit_in_milli: u32,
        ram_request_in_mib: u32,
        ram_limit_in_mib: u32,
        persistent_storage: PersistentStorage,
        build: Build,
        root_module_path: PathBuf,
        terraform_files_source: TerraformFilesSource,
        terraform_var_file_paths: Vec<String>,
        terraform_vars: Vec<(String, String)>,
        backend: TerraformBackend,
        terraform_action: TerraformAction,
        timeout: Duration,
        environment_variables: HashMap<String, VariableInfo>,
        advanced_settings: TerraformServiceAdvancedSettings,
        mk_event_details: impl Fn(Transmitter) -> EventDetails,
        annotations_groups: Vec<AnnotationsGroup>,
        labels_groups: Vec<LabelsGroup>,
        terraform_credentials: TerraformCredentials,
    ) -> Result<Self, TerraformServiceError> {
        let event_details = mk_event_details(Transmitter::TerraformService(long_id, name.clone()));
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
            deployment_id: context
                .execution_id()
                .rsplit_once('-')
                .map(|s| s.0.to_string())
                .unwrap_or_default(),
            name,
            kube_name,
            action,
            build,
            root_module_path,
            terraform_files_source,
            terraform_var_file_paths,
            terraform_vars,
            backend,
            terraform_action,
            timeout,
            cpu_request: KubernetesCpuResourceUnit::MilliCpu(cpu_request_in_milli),
            cpu_limit: KubernetesCpuResourceUnit::MilliCpu(cpu_limit_in_milli),
            ram_request: KubernetesMemoryResourceUnit::MebiByte(ram_request_in_mib),
            ram_limit: KubernetesMemoryResourceUnit::MebiByte(ram_limit_in_mib),
            persistent_storage,
            environment_variables,
            advanced_settings,
            annotations_group: AnnotationsGroupTeraContext::new(annotations_groups),
            labels_group: LabelsGroupTeraContext::new(labels_groups),
            workspace_directory,
            lib_root_directory: context.lib_root_dir().to_string(),
            terraform_credentials,
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
        let environment_variables = add_cloud_provider_credentials_if_necessary(
            self.get_environment_variables(),
            &self.terraform_credentials,
            &target.cloud_provider.credentials_environment_variables(),
        );

        let environment = target.environment;
        let (image_full, image_name, image_tag) = match &self.terraform_files_source {
            TerraformFilesSource::Git { .. } => (
                self.build.image.full_image_name_with_tag(),
                self.build.image.name.to_string(),
                self.build.image.tag.clone(),
            ),
        };

        let mut deployment_affinity_node_required = utils::add_arch_to_deployment_affinity_node(
            &self.advanced_settings.deployment_affinity_node_required,
            &target.kubernetes.cpu_architectures(),
        );

        if target.cloud_provider.kind() == Kind::Aws && !target.kubernetes.is_karpenter_enabled() {
            // For AWS cluster, when Karpenter is not enabled, then force the pod to always run on the same zone.
            // There is a bug where the node auto-scaler is not starting a node in the same zone of the Persistent Volume.
            deployment_affinity_node_required
                .entry("topology.kubernetes.io/zone".to_string())
                .or_insert_with(|| format!("{}a", target.kubernetes.region()));
        }

        let mut adv_settings = self.advanced_settings.clone();
        adv_settings.deployment_affinity_node_required = deployment_affinity_node_required;

        let backend_config = self
            .backend
            .configs
            .iter()
            .map(|config| config.0.clone())
            .collect::<Vec<_>>();

        let kubernetes = target.kubernetes;
        let registry_info = target.container_registry.registry_info();

        let command_args = self.get_command_args();

        TerraformServiceTeraContext {
            organization_long_id: environment.organization_long_id,
            project_long_id: environment.project_long_id,
            environment_short_id: to_short_id(&environment.long_id),
            environment_long_id: environment.long_id,
            deployment_id: self.deployment_id.to_string(),
            namespace: environment.namespace().to_string(),
            service: ServiceTeraContext {
                short_id: to_short_id(&self.long_id),
                long_id: self.long_id,
                name: self.kube_name.clone(),
                image_full,
                image_tag,
                version: self.service_version(),
                job_max_duration_in_sec: self.timeout.as_secs(),
                advanced_settings: adv_settings,
                entrypoint: "entrypoint.sh".to_string(),
                command_args,
                cpu_request_in_milli: self.cpu_request.to_string(),
                cpu_limit_in_milli: self.cpu_limit.to_string(),
                ram_request_in_mib: self.ram_request.to_string(),
                ram_limit_in_mib: self.ram_limit.to_string(),
                // max_nb_restart: self.max_nb_restart,
                // max_duration_in_sec: self.max_duration.as_secs(),
                persistence_size_in_gib: self.persistent_storage.size_in_gib.to_string(),
                persistence_storage_type: self.persistent_storage.storage_class.clone(),
            },
            registry: registry_info
                .get_registry_docker_json_config(DockerRegistryInfo {
                    registry_name: Some(kubernetes.cluster_name()), // TODO(benjaminch): this is a bit of a hack, considering registry name will be the same as cluster one, it should be the case, but worth doing it better
                    repository_name: None,
                    image_name: Some(image_name),
                })
                .as_ref()
                .map(|docker_json| RegistryTeraContext {
                    secret_name: format!("{}-registry", self.kube_name()),
                    docker_json_config: Some(docker_json.to_string()),
                }),
            annotations_group: self.annotations_group.clone(),
            labels_group: self.labels_group.clone(),
            environment_variables,
            backend_config: BackendConfigTeraContext {
                secret_name: self.backend.kube_secret_name.to_owned(),
                configs: backend_config,
            },
        }
    }

    fn get_command_args(&self) -> Vec<String> {
        let base_path = self.root_module_path.to_str().unwrap_or_default().to_string();

        let var_file_args: Vec<String> = self
            .terraform_var_file_paths
            .iter()
            .map(|path| format!("-var-file={path}"))
            .collect();

        let var_args: Vec<String> = self
            .terraform_vars
            .iter()
            .flat_map(|(key, value)| {
                let arg = "-var".to_string();
                let val = format!("{key}={value}");

                vec![arg, val]
            })
            .collect();

        match &self.terraform_action {
            TerraformAction::TerraformPlanOnly { execution_id } => {
                let mut args = vec![base_path, "plan_only".to_string(), execution_id.clone()];
                args.extend(var_file_args);
                args.extend(var_args);
                args
            }
            TerraformAction::TerraformPlanAndApply => {
                let mut args = vec![base_path, "apply".to_string(), String::new()];
                args.extend(var_file_args);
                args.extend(var_args);
                args
            }
            TerraformAction::TerraformApplyFromPlan { execution_id } => {
                let mut args = vec![base_path, "apply_from_plan".to_string(), execution_id.clone()];
                args.extend(var_file_args);
                args.extend(var_args);
                args
            }
            TerraformAction::TerraformDestroy => {
                let mut args = vec![base_path, "destroy".to_string(), String::new()];
                args.extend(var_file_args);
                args.extend(var_args);
                args
            }
        }
    }
}

fn add_cloud_provider_credentials_if_necessary(
    mut existing_vars: Vec<EnvironmentVariable>,
    terraform_credentials: &TerraformCredentials,
    credential_vars: &[(&str, &str)],
) -> Vec<EnvironmentVariable> {
    if terraform_credentials.use_cluster_credentials {
        let encoded_credentials = credential_vars.iter().map(|(key, value)| EnvironmentVariable {
            key: (*key).to_string(),
            value: general_purpose::STANDARD.encode(value),
            is_secret: true,
        });

        existing_vars.extend(encoded_credentials);
    }

    existing_vars
}

impl<T: CloudProvider> Service for TerraformService<T> {
    fn service_type(&self) -> ServiceType {
        ServiceType::Terraform
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
    TerraformPlanOnly { execution_id: String },
    TerraformPlanAndApply,
    TerraformApplyFromPlan { execution_id: String },
    TerraformDestroy,
}

pub struct TerraformBackendConfig(String);
impl FromStr for TerraformBackendConfig {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if !s.contains('=') {
            return Err(format!("Invalid backend_config. Expected <key>=<value>: {s}"));
        }
        Ok(TerraformBackendConfig(s.to_owned()))
    }
}

pub struct TerraformBackend {
    pub configs: Vec<TerraformBackendConfig>,
    pub kube_secret_name: String,
}

pub struct PersistentStorage {
    pub storage_class: String,
    pub size_in_gib: KubernetesMemoryResourceUnit,
}

pub struct TerraformCredentials {
    pub use_cluster_credentials: bool,
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
pub(crate) struct BackendConfigTeraContext {
    pub(crate) secret_name: String,
    pub(crate) configs: Vec<String>,
}

#[derive(Serialize, Debug, Clone)]
pub(crate) struct TerraformServiceTeraContext {
    pub(crate) organization_long_id: Uuid,
    pub(crate) project_long_id: Uuid,
    pub(crate) environment_short_id: String,
    pub(crate) environment_long_id: Uuid,
    pub(crate) deployment_id: String,
    pub(crate) namespace: String,
    pub(crate) service: ServiceTeraContext,
    pub(crate) registry: Option<RegistryTeraContext>,
    pub(crate) annotations_group: AnnotationsGroupTeraContext,
    pub(crate) labels_group: LabelsGroupTeraContext,
    pub(crate) environment_variables: Vec<EnvironmentVariable>,
    pub(crate) backend_config: BackendConfigTeraContext,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_credentials_when_flag_is_true() {
        let existing = vec![EnvironmentVariable {
            key: "EXISTING".to_string(),
            value: "value".to_string(),
            is_secret: false,
        }];

        let credentials = TerraformCredentials {
            use_cluster_credentials: true,
        };

        let credential_vars = vec![("AWS_ACCESS_KEY_ID", "AKIA..."), ("AWS_SECRET_ACCESS_KEY", "secret123")];

        let result = add_cloud_provider_credentials_if_necessary(existing.clone(), &credentials, &credential_vars);

        assert_eq!(result.len(), 3);

        assert!(result.contains(&EnvironmentVariable {
            key: "EXISTING".to_string(),
            value: "value".to_string(),
            is_secret: false,
        }));

        assert!(result.contains(&EnvironmentVariable {
            key: "AWS_ACCESS_KEY_ID".to_string(),
            value: base64::encode("AKIA..."),
            is_secret: true,
        }));

        assert!(result.contains(&EnvironmentVariable {
            key: "AWS_SECRET_ACCESS_KEY".to_string(),
            value: base64::encode("secret123"),
            is_secret: true,
        }));
    }

    #[test]
    fn test_do_not_add_credentials_when_flag_is_false() {
        let existing = vec![EnvironmentVariable {
            key: "EXISTING".to_string(),
            value: "value".to_string(),
            is_secret: false,
        }];

        let credentials = TerraformCredentials {
            use_cluster_credentials: false,
        };

        let credential_vars = vec![("AWS_ACCESS_KEY_ID", "AKIA..."), ("AWS_SECRET_ACCESS_KEY", "secret123")];

        let result = add_cloud_provider_credentials_if_necessary(existing.clone(), &credentials, &credential_vars);

        assert_eq!(result, existing);
    }

    #[test]
    fn test_empty_existing_and_add_credentials() {
        let existing = vec![];

        let credentials = TerraformCredentials {
            use_cluster_credentials: true,
        };

        let credential_vars = vec![("FOO", "bar")];

        let result = add_cloud_provider_credentials_if_necessary(existing, &credentials, &credential_vars);

        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0],
            EnvironmentVariable {
                key: "FOO".to_string(),
                value: base64::encode("bar"),
                is_secret: true,
            }
        );
    }

    #[test]
    fn test_empty_existing_and_no_credentials_added() {
        let existing = vec![];

        let credentials = TerraformCredentials {
            use_cluster_credentials: false,
        };

        let credential_vars = vec![("FOO", "bar")];

        let result = add_cloud_provider_credentials_if_necessary(existing.clone(), &credentials, &credential_vars);

        assert!(result.is_empty());
    }
}

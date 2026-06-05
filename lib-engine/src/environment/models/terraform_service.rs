use crate::environment::action::DeploymentAction;
use crate::environment::models::annotations_group::AnnotationsGroupTeraContext;
use crate::environment::models::container::{ClusterTeraContext, RegistryTeraContext};
use crate::environment::models::external_secret::{ExternalSecretGroup, build_external_secret_groups};
use crate::environment::models::labels_group::LabelsGroupTeraContext;
use crate::environment::models::types::CloudProvider;
use crate::environment::models::utils;
use crate::events::{EventDetails, Stage, Transmitter};
use crate::infrastructure::models::build_platform::{Build, Credentials, SshKey};
use crate::infrastructure::models::cloud_provider::service::{Action, Service, ServiceType};
use crate::infrastructure::models::cloud_provider::{DeploymentTarget, Kind};
use crate::infrastructure::models::container_registry::DockerRegistryInfo;
use crate::infrastructure::models::kubernetes::karpenter::KarpenterNodePoolType;
use crate::io_models::annotations_group::AnnotationsGroup;
use crate::io_models::context::Context;
use crate::io_models::labels_group::LabelsGroup;
use crate::io_models::models::ExternalSecret;
use crate::io_models::models::{
    EnvironmentVariable, KubernetesCpuResourceUnit, KubernetesGpuResourceUnit, KubernetesMemoryResourceUnit,
};
use crate::io_models::terraform::TerraformServiceAdvancedSettings;
use crate::io_models::variable_utils::VariableInfo;
use crate::utilities::{sanitize_k8s_label_value, to_short_id};
use base64::Engine;
use base64::engine::general_purpose;
use itertools::Itertools;
use serde_derive::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
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
    pub(crate) gpu_request: Option<KubernetesGpuResourceUnit>,
    pub(crate) gpu_limit: Option<KubernetesGpuResourceUnit>,
    pub(crate) persistent_storage: PersistentStorage,
    pub(crate) environment_variables: HashMap<String, VariableInfo>,
    pub(crate) extra_action_arguments: BTreeMap<String, Vec<String>>,
    pub(crate) advanced_settings: TerraformServiceAdvancedSettings,
    pub(crate) annotations_group: AnnotationsGroupTeraContext,
    pub(crate) labels_group: LabelsGroupTeraContext,
    pub(crate) workspace_directory: PathBuf,
    pub(crate) lib_root_directory: String,
    pub(crate) terraform_credentials: TerraformCredentials,
    pub(crate) external_secrets: Vec<ExternalSecretGroup>,
}

impl<T: CloudProvider> TerraformService<T> {
    pub fn new(
        context: &Context,
        long_id: Uuid,
        name: String,
        kube_name: String,
        action: Action,
        cpu_request: KubernetesCpuResourceUnit,
        cpu_limit: KubernetesCpuResourceUnit,
        ram_request: KubernetesMemoryResourceUnit,
        ram_limit: KubernetesMemoryResourceUnit,
        gpu_request: Option<KubernetesGpuResourceUnit>,
        gpu_limit: Option<KubernetesGpuResourceUnit>,
        persistent_storage: PersistentStorage,
        build: Build,
        terraform_files_source: TerraformFilesSource,
        terraform_var_file_paths: Vec<String>,
        terraform_vars: Vec<(String, String)>,
        backend: TerraformBackend,
        terraform_action: TerraformAction,
        timeout: Duration,
        environment_variables: HashMap<String, VariableInfo>,
        extra_action_arguments: BTreeMap<String, Vec<String>>,
        advanced_settings: TerraformServiceAdvancedSettings,
        mk_event_details: impl Fn(Transmitter) -> EventDetails,
        annotations_groups: Vec<AnnotationsGroup>,
        labels_groups: Vec<LabelsGroup>,
        terraform_credentials: TerraformCredentials,
        external_secrets: BTreeMap<String, ExternalSecret>,
    ) -> Result<Self, TerraformServiceError> {
        let event_details = mk_event_details(Transmitter::TerraformService(long_id, name.clone()));
        let mk_event_details = move |stage: Stage| EventDetails::clone_changing_stage(event_details.clone(), stage);
        let external_secrets = build_external_secret_groups(&kube_name, external_secrets);

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
            terraform_files_source,
            terraform_var_file_paths,
            terraform_vars,
            backend,
            terraform_action,
            timeout,
            cpu_request,
            cpu_limit,
            ram_request,
            ram_limit,
            gpu_request,
            gpu_limit,
            persistent_storage,
            environment_variables,
            extra_action_arguments,
            advanced_settings,
            annotations_group: AnnotationsGroupTeraContext::new(annotations_groups),
            labels_group: LabelsGroupTeraContext::new(labels_groups),
            workspace_directory,
            lib_root_directory: context.lib_root_dir().to_string(),
            terraform_credentials,
            external_secrets,
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
        self.kube_name.clone()
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
        let mut tolerations = BTreeMap::<String, String>::new();
        let is_gpu = (self.gpu_request.is_some_and(|v| v.to_gpu_count() > 0))
            || (self.gpu_limit.is_some_and(|v| v.to_gpu_count() > 0));

        if target.cloud_provider.kind() == Kind::Aws && !target.kubernetes.is_karpenter_enabled() {
            // For AWS cluster, when Karpenter is not enabled, then force the pod to always run on the same zone.
            // There is a bug where the node auto-scaler is not starting a node in the same zone of the Persistent Volume.
            deployment_affinity_node_required
                .entry("topology.kubernetes.io/zone".to_string())
                .or_insert_with(|| format!("{}a", target.kubernetes.region()));
        }

        if !is_gpu && target.kubernetes.is_karpenter_cronjob_nodepool_enabled() {
            utils::target_karpenter_node_pool(
                KarpenterNodePoolType::Cronjob,
                &mut deployment_affinity_node_required,
                &mut tolerations,
                false,
            );
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
            cluster: ClusterTeraContext::from(kubernetes),
            namespace: environment.namespace().to_string(),
            service: ServiceTeraContext {
                short_id: to_short_id(&self.long_id),
                long_id: self.long_id,
                name: self.kube_name.clone(),
                image_full,
                image_tag_label: sanitize_k8s_label_value(&image_tag),
                image_tag,
                version: self.service_version(),
                job_max_duration_in_sec: self.timeout.as_secs(),
                advanced_settings: adv_settings,
                entrypoint: "entrypoint.sh".to_string(),
                command_args,
                tolerations,
                cpu_request_in_milli: self.cpu_request.to_string(),
                cpu_limit_in_milli: self.cpu_limit.to_string(),
                ram_request_in_mib: self.ram_request.to_string(),
                ram_limit_in_mib: self.ram_limit.to_string(),
                gpu_request: self.gpu_request.map(u32::from),
                gpu_limit: self.gpu_limit.map(u32::from),
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
            external_secrets: self.external_secrets.clone(),
            backend_config: BackendConfigTeraContext {
                secret_name: self.backend.kube_secret_name.to_owned(),
                configs: backend_config,
            },
        }
    }

    fn get_command_args(&self) -> Vec<String> {
        // Pass root_module_path so entrypoint navigates to the correct module directory
        let base_path = match &self.terraform_files_source {
            TerraformFilesSource::Git { root_module_path, .. } => root_module_path.trim_start_matches('/').to_string(),
        };

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
            TerraformAction::TerraformUnlockState => {
                let args = vec![base_path, "unlock_state".to_string(), String::new()];
                args
            }
            TerraformAction::TerraformInit => {
                let args = vec![base_path, "init".to_string(), String::new()];
                args
            }
            TerraformAction::TerraformNoop => {
                // No command args needed since it's a noop
                vec![]
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
        match is_terraform_noop(&self.terraform_action) {
            true => None,
            false => Some(&self.build),
        }
    }

    fn build_mut(&mut self) -> Option<&mut Build> {
        match is_terraform_noop(&self.terraform_action) {
            true => None,
            false => Some(&mut self.build),
        }
    }

    fn get_environment_variables(&self) -> Vec<EnvironmentVariable> {
        let env_vars = self
            .environment_variables
            .iter()
            .map(|(key, variable_infos)| EnvironmentVariable {
                key: key.clone(),
                value: variable_infos.value.clone(),
                is_secret: variable_infos.is_secret,
            });

        // https://developer.hashicorp.com/terraform/cli/config/environment-variables#tf_cli_args-and-tf_cli_args_name
        let extra_actions = self
            .extra_action_arguments
            .iter()
            .map(|(key, vals)| EnvironmentVariable {
                key: format!("TF_CLI_ARGS_{key}"),
                value: base64::engine::general_purpose::STANDARD.encode(vals.iter().join(" ")),
                is_secret: false,
            });

        env_vars.chain(extra_actions).collect()
    }
}

pub trait TerraformServiceTrait: Service + DeploymentAction + Send {
    fn advanced_settings(&self) -> &TerraformServiceAdvancedSettings;
    fn as_deployment_action(&self) -> &dyn DeploymentAction;
    fn job_max_duration(&self) -> &Duration;
    fn external_secrets(&self) -> &[ExternalSecretGroup];
    fn external_secrets_mut(&mut self) -> &mut [ExternalSecretGroup];
    fn lib_root_directory(&self) -> &str;
    fn workspace_directory_path(&self) -> &Path;
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

    fn external_secrets(&self) -> &[ExternalSecretGroup] {
        &self.external_secrets
    }

    fn external_secrets_mut(&mut self) -> &mut [ExternalSecretGroup] {
        &mut self.external_secrets
    }

    fn lib_root_directory(&self) -> &str {
        &self.lib_root_directory
    }

    fn workspace_directory_path(&self) -> &Path {
        Path::new(TerraformService::workspace_directory(self))
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
    TerraformUnlockState,
    TerraformInit,
    TerraformNoop,
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
    pub(crate) image_tag_label: String,
    pub(crate) version: String,
    pub(crate) job_max_duration_in_sec: u64,
    pub(crate) cpu_request_in_milli: String,
    pub(crate) cpu_limit_in_milli: String,
    pub(crate) ram_request_in_mib: String,
    pub(crate) ram_limit_in_mib: String,
    pub(crate) gpu_request: Option<u32>,
    pub(crate) gpu_limit: Option<u32>,
    pub(crate) advanced_settings: TerraformServiceAdvancedSettings,
    pub(crate) entrypoint: String,
    pub(crate) command_args: Vec<String>,
    pub(crate) tolerations: BTreeMap<String, String>,
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
    pub(crate) cluster: ClusterTeraContext,
    pub(crate) namespace: String,
    pub(crate) service: ServiceTeraContext,
    pub(crate) registry: Option<RegistryTeraContext>,
    pub(crate) annotations_group: AnnotationsGroupTeraContext,
    pub(crate) labels_group: LabelsGroupTeraContext,
    pub(crate) environment_variables: Vec<EnvironmentVariable>,
    pub(crate) external_secrets: Vec<ExternalSecretGroup>,
    pub(crate) backend_config: BackendConfigTeraContext,
}

pub(crate) fn is_terraform_noop(action: &TerraformAction) -> bool {
    matches!(action, TerraformAction::TerraformNoop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environment::models::annotations_group::AnnotationsGroupTeraContext;
    use crate::environment::models::labels_group::LabelsGroupTeraContext;
    use tera::{Context, Tera};

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

    #[test]
    fn test_get_command_args_base_path_strips_leading_slash() {
        // Test the logic used in get_command_args() to derive base_path from root_module_path.
        // The entrypoint receives base_path as its first argument and uses it to cd into the
        // correct module directory inside the container (after the build context is the repo root).
        let test_cases = vec![
            ("/modules/root", "modules/root"),
            ("modules/root", "modules/root"),
            (".", "."),
            ("/nested/a/b", "nested/a/b"),
        ];

        for (input, expected) in test_cases {
            let source = TerraformFilesSource::Git {
                git_url: Url::parse("https://github.com/test/repo").unwrap(),
                get_credentials: Box::new(|| Ok(None)),
                commit_id: "abc123".to_string(),
                root_module_path: input.to_string(),
                ssh_keys: vec![],
            };

            // This is the exact logic used in get_command_args()
            let base_path = match &source {
                TerraformFilesSource::Git { root_module_path, .. } => {
                    root_module_path.trim_start_matches('/').to_string()
                }
            };

            assert_eq!(
                base_path, expected,
                "root_module_path '{input}' should produce base_path '{expected}'"
            );
        }
    }

    #[test]
    fn test_noop_terraform_action_does_not_require_build() {
        assert!(is_terraform_noop(&TerraformAction::TerraformNoop));
    }

    #[test]
    fn test_non_noop_terraform_actions_require_build() {
        assert!(!is_terraform_noop(&TerraformAction::TerraformPlanAndApply));
        assert!(!is_terraform_noop(&TerraformAction::TerraformDestroy));
        assert!(!is_terraform_noop(&TerraformAction::TerraformPlanOnly {
            execution_id: "exec-1".to_string()
        }));
        assert!(!is_terraform_noop(&TerraformAction::TerraformInit));
    }

    #[test]
    fn renders_terraform_job_template_with_required_cronjob_nodepool_affinity() {
        let mut tolerations = BTreeMap::new();
        tolerations.insert("nodepool/cronjob".to_string(), "NoSchedule".to_string());

        let mut affinity_required = BTreeMap::new();
        affinity_required.insert("karpenter.sh/nodepool".to_string(), "cronjob".to_string());

        let advanced_settings = TerraformServiceAdvancedSettings {
            deployment_affinity_node_required: affinity_required,
            ..Default::default()
        };

        let rendered = Tera::one_off(
            include_str!("../../../lib/common/charts/q-terraform-service/templates/job.j2.yaml"),
            &Context::from_serialize(TerraformServiceTeraContext {
                organization_long_id: Uuid::new_v4(),
                project_long_id: Uuid::new_v4(),
                environment_short_id: "env123456".to_string(),
                environment_long_id: Uuid::new_v4(),
                deployment_id: "deploy123456".to_string(),
                cluster: ClusterTeraContext {
                    long_id: Uuid::new_v4(),
                    name: "test-cluster".to_string(),
                    region: "us-east-1".to_string(),
                    zone: "us-east-1a".to_string(),
                    is_karpenter_enabled: true,
                },
                namespace: "test-namespace".to_string(),
                service: ServiceTeraContext {
                    short_id: "tf123456".to_string(),
                    long_id: Uuid::new_v4(),
                    name: "test-terraform-job".to_string(),
                    image_full: "registry.example.com/test-image:latest".to_string(),
                    image_tag: "latest".to_string(),
                    image_tag_label: "latest".to_string(),
                    version: "test-image:latest".to_string(),
                    job_max_duration_in_sec: 120,
                    cpu_request_in_milli: "250m".to_string(),
                    cpu_limit_in_milli: "250m".to_string(),
                    ram_request_in_mib: "256Mi".to_string(),
                    ram_limit_in_mib: "256Mi".to_string(),
                    gpu_request: None,
                    gpu_limit: None,
                    advanced_settings,
                    entrypoint: "entrypoint.sh".to_string(),
                    command_args: vec!["plan".to_string()],
                    tolerations,
                    persistence_size_in_gib: "10Gi".to_string(),
                    persistence_storage_type: "gp3".to_string(),
                },
                registry: None,
                annotations_group: AnnotationsGroupTeraContext::new(vec![]),
                labels_group: LabelsGroupTeraContext::new(vec![]),
                environment_variables: vec![],
                external_secrets: vec![],
                backend_config: BackendConfigTeraContext {
                    secret_name: "backend-config".to_string(),
                    configs: vec![],
                },
            })
            .expect("terraform tera context should serialize"),
            false,
        )
        .expect("template should render");

        assert!(rendered.contains("requiredDuringSchedulingIgnoredDuringExecution"));
        assert!(rendered.contains("karpenter.sh/nodepool"));
        assert!(rendered.contains("- cronjob"));
        assert!(rendered.contains("key: \"nodepool/cronjob\""));
        assert!(!rendered.contains("preferredDuringSchedulingIgnoredDuringExecution"));
        assert!(rendered.contains("karpenter.sh/do-not-disrupt"));
    }

    fn minimal_pdb_context(is_karpenter_enabled: bool) -> TerraformServiceTeraContext {
        TerraformServiceTeraContext {
            organization_long_id: Uuid::new_v4(),
            project_long_id: Uuid::new_v4(),
            environment_short_id: "env123456".to_string(),
            environment_long_id: Uuid::new_v4(),
            deployment_id: "deploy123456".to_string(),
            cluster: ClusterTeraContext {
                long_id: Uuid::new_v4(),
                name: "test-cluster".to_string(),
                region: "us-east-1".to_string(),
                zone: "us-east-1a".to_string(),
                is_karpenter_enabled,
            },
            namespace: "test-namespace".to_string(),
            service: ServiceTeraContext {
                short_id: "tf123456".to_string(),
                long_id: Uuid::new_v4(),
                name: "test-terraform-job".to_string(),
                image_full: "registry.example.com/test-image:latest".to_string(),
                image_tag: "latest".to_string(),
                image_tag_label: "latest".to_string(),
                version: "test-image:latest".to_string(),
                job_max_duration_in_sec: 120,
                cpu_request_in_milli: "250m".to_string(),
                cpu_limit_in_milli: "250m".to_string(),
                ram_request_in_mib: "256Mi".to_string(),
                ram_limit_in_mib: "256Mi".to_string(),
                gpu_request: None,
                gpu_limit: None,
                advanced_settings: TerraformServiceAdvancedSettings::default(),
                entrypoint: "entrypoint.sh".to_string(),
                command_args: vec![],
                tolerations: BTreeMap::new(),
                persistence_size_in_gib: "10Gi".to_string(),
                persistence_storage_type: "gp3".to_string(),
            },
            registry: None,
            annotations_group: AnnotationsGroupTeraContext::new(vec![]),
            labels_group: LabelsGroupTeraContext::new(vec![]),
            environment_variables: vec![],
            external_secrets: vec![],
            backend_config: BackendConfigTeraContext {
                secret_name: "backend-config".to_string(),
                configs: vec![],
            },
        }
    }

    #[test]
    fn pdb_template_rendered_when_karpenter_disabled() {
        let rendered = Tera::one_off(
            include_str!("../../../lib/common/charts/q-terraform-service/templates/pdb.j2.yaml"),
            &Context::from_serialize(minimal_pdb_context(false)).expect("should serialize"),
            false,
        )
        .expect("template should render");

        assert!(rendered.contains("PodDisruptionBudget"));
        assert!(rendered.contains("terraform-service"));
        assert!(rendered.contains("minAvailable: 1"));
    }

    #[test]
    fn pdb_template_not_rendered_when_karpenter_enabled() {
        let rendered = Tera::one_off(
            include_str!("../../../lib/common/charts/q-terraform-service/templates/pdb.j2.yaml"),
            &Context::from_serialize(minimal_pdb_context(true)).expect("should serialize"),
            false,
        )
        .expect("template should render");

        assert!(!rendered.contains("PodDisruptionBudget"));
    }
}

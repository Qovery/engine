use crate::engine_task::qovery_api::QoveryApi;
use crate::environment::models;
use crate::environment::models::terraform_service::{TerraformServiceError, TerraformServiceTrait};
use crate::environment::models::types::{AWS, Azure, GCP, OnPremise, SCW};
use crate::infrastructure::models::build_platform::{Build, GitRepository, GitRepositoryExtraFile, Image, SshKey};
use crate::infrastructure::models::cloud_provider::CloudProvider;
use crate::infrastructure::models::cloud_provider::service::ServiceType;
use crate::infrastructure::models::container_registry::{
    ContainerRegistryInfo, DockerRegistryInfo, InteractWithRegistry,
};
use crate::infrastructure::models::kubernetes::{Kind, Kubernetes};
use crate::io_models::annotations_group::AnnotationsGroup;
use crate::io_models::application::GitCredentials;
use crate::io_models::context::Context;
use crate::io_models::labels_group::LabelsGroup;
use crate::io_models::models::{
    CpuArchitecture, KubernetesCpuResourceUnit, KubernetesGpuResourceUnit, KubernetesMemoryResourceUnit,
};
use crate::io_models::variable_utils::{VariableInfo, default_environment_vars_with_info};
use crate::io_models::{
    Action, QoveryIdentifier, fetch_git_token, normalize_root_and_dockerfile_path, sanitized_git_url,
    ssh_keys_from_env_vars,
};
use crate::utilities::to_short_id;
use base64::Engine;
use base64::engine::general_purpose;
use itertools::Itertools;
use serde_derive::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use url::Url;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
#[serde(default)]
pub struct TerraformServiceAdvancedSettings {
    // Deployment
    #[serde(alias = "deployment.termination_grace_period_seconds")]
    pub deployment_termination_grace_period_seconds: u32,
    #[serde(alias = "deployment.affinity.node.required")]
    pub deployment_affinity_node_required: BTreeMap<String, String>,

    // Build
    #[serde(alias = "build.timeout_max_sec")]
    pub build_timeout_max_sec: u32,
    #[serde(alias = "build.cpu_max_in_milli")]
    pub build_cpu_max_in_milli: u32,
    #[serde(alias = "build.ram_max_in_gib")]
    pub build_ram_max_in_gib: u32,
    #[serde(default, alias = "build.ephemeral_storage_in_gib")]
    pub build_ephemeral_storage_in_gib: Option<u32>,

    #[serde(alias = "security.service_account_name")]
    pub security_service_account_name: String,
    #[serde(alias = "security.read_only_root_filesystem")]
    pub security_read_only_root_filesystem: bool,
    #[serde(alias = "security.automount_service_account_token")]
    pub security_automount_service_account_token: bool,
}

impl Default for TerraformServiceAdvancedSettings {
    fn default() -> Self {
        TerraformServiceAdvancedSettings {
            deployment_termination_grace_period_seconds: 60,
            deployment_affinity_node_required: BTreeMap::new(),
            build_timeout_max_sec: 30 * 60,
            build_cpu_max_in_milli: 4000,
            build_ram_max_in_gib: 8,
            build_ephemeral_storage_in_gib: None,
            security_service_account_name: "".to_string(),
            security_read_only_root_filesystem: false,
            security_automount_service_account_token: false,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TerraformFilesSource {
    Git {
        git_url: Url,
        git_credentials: Option<GitCredentials>,
        commit_id: String,
        root_module_path: String,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TerraformProvider {
    Terraform,
    OpenTofu,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
pub struct TerraformBackend {
    pub backend_type: TerraformBackendType,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TerraformBackendType {
    DefinedInTerraformFile,
    Kubernetes,
}

impl TerraformBackendType {
    fn to_backend_block_name(&self) -> &'static str {
        match self {
            TerraformBackendType::DefinedInTerraformFile => "invalid",
            TerraformBackendType::Kubernetes => "kubernetes",
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TerraformActionCommand {
    PlanOnly,
    PlanAndApply,
    ApplyFromPlan,
    Destroy,
    ForceUnlockState,
    Init, // Used to migrate the state of the user with init -migrate-state -force-copy
    Noop,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
pub struct TerraformAction {
    pub command: TerraformActionCommand,
    pub plan_execution_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct PersistentStorage {
    pub storage_class: String,
    pub size_in_gib: u32,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct TerraformCredentials {
    pub use_cluster_credentials: bool,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct TerraformService {
    pub long_id: Uuid,
    pub name: String,
    pub kube_name: String,
    pub action: Action,
    pub cpu_request_in_milli: u32,
    pub cpu_limit_in_milli: u32,
    pub ram_request_in_mib: u32,
    pub ram_limit_in_mib: u32,
    pub gpu_request: Option<u32>,
    pub gpu_limit: Option<u32>,
    pub persistent_storage: PersistentStorage,
    pub tf_files_source: TerraformFilesSource,
    pub tf_var_file_paths: Vec<String>,
    pub tf_vars: Vec<(String, String)>,
    pub provider: TerraformProvider,
    pub provider_version: String,
    pub backend: TerraformBackend,
    pub terraform_action: TerraformAction,
    pub timeout_sec: u64,

    /// Key is a String, Value is a base64 encoded String
    /// Use BTreeMap to get Hash trait which is not available on HashMap
    #[serde(default = "default_environment_vars_with_info")]
    pub environment_vars_with_infos: BTreeMap<String, VariableInfo>,
    #[serde(default)]
    pub advanced_settings: TerraformServiceAdvancedSettings,
    #[serde(default)]
    pub annotations_group_ids: BTreeSet<Uuid>,
    #[serde(default)]
    pub labels_group_ids: BTreeSet<Uuid>,

    #[serde(default)] // Default is false
    pub shared_image_feature_enabled: bool,
    pub terraform_credentials: Option<TerraformCredentials>,
    // key is the name of the terraform action I.e: apply. Value is the list of extra arguments for this action
    #[serde(default)]
    pub extra_action_arguments: BTreeMap<String, Vec<String>>,
}

impl TerraformService {
    pub fn to_terraform_service_domain(
        self,
        context: &Context,
        cloud_provider: &dyn CloudProvider,
        default_container_registry: &dyn InteractWithRegistry,
        cluster: &dyn Kubernetes,
        environment_kube_name: &str,
        environment_long_id: Uuid,
        annotations_group: &BTreeMap<Uuid, AnnotationsGroup>,
        labels_group: &BTreeMap<Uuid, LabelsGroup>,
    ) -> Result<Box<dyn TerraformServiceTrait>, TerraformServiceError> {
        // Get passphrase and public key if provided by the user
        let ssh_keys = ssh_keys_from_env_vars(&self.environment_vars_with_infos);
        let environment_variables_with_info: HashMap<String, VariableInfo> = self
            .environment_vars_with_infos
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let annotations_groups = self
            .annotations_group_ids
            .iter()
            .filter_map(|id| annotations_group.get(id))
            .cloned()
            .collect_vec();

        let labels_groups = self
            .labels_group_ids
            .iter()
            .filter_map(|id| labels_group.get(id))
            .cloned()
            .collect_vec();

        let build = self.build_for_terraform_service(
            &ssh_keys,
            context.qovery_api.clone(),
            default_container_registry.registry_info(),
            cluster.cpu_architectures(),
            &QoveryIdentifier::new(*cluster.long_id()),
        )?;

        let tf_files_source_domain =
            self.get_terraform_files_source_domain(&ssh_keys, context.qovery_api.clone(), self.long_id);

        let backend = self.get_terraform_backend(environment_kube_name, environment_long_id)?;

        let terraform_action = self.get_terraform_action()?;

        let persistent_storage = models::terraform_service::PersistentStorage {
            storage_class: self.persistent_storage.storage_class.clone(),
            size_in_gib: KubernetesMemoryResourceUnit::GibiByte(self.persistent_storage.size_in_gib),
        };

        let terraform_credentials_domain = self.get_terraform_credentials_domain()?;

        let service: Box<dyn TerraformServiceTrait> = match cloud_provider.kubernetes_kind() {
            Kind::Eks | Kind::EksSelfManaged | Kind::EksAnywhere => {
                Box::new(models::terraform_service::TerraformService::<AWS>::new(
                    context,
                    self.long_id,
                    self.name,
                    self.kube_name,
                    self.action.to_service_action(),
                    KubernetesCpuResourceUnit::MilliCpu(self.cpu_request_in_milli),
                    KubernetesCpuResourceUnit::MilliCpu(self.cpu_limit_in_milli),
                    KubernetesMemoryResourceUnit::MebiByte(self.ram_request_in_mib),
                    KubernetesMemoryResourceUnit::MebiByte(self.ram_limit_in_mib),
                    self.gpu_request.map(KubernetesGpuResourceUnit),
                    self.gpu_limit.map(KubernetesGpuResourceUnit),
                    persistent_storage,
                    build,
                    tf_files_source_domain,
                    self.tf_var_file_paths,
                    self.tf_vars,
                    backend,
                    terraform_action,
                    Duration::from_secs(self.timeout_sec),
                    environment_variables_with_info,
                    self.extra_action_arguments,
                    self.advanced_settings,
                    |transmitter| context.get_event_details(transmitter),
                    annotations_groups,
                    labels_groups,
                    terraform_credentials_domain,
                )?)
            }
            Kind::ScwKapsule | Kind::ScwSelfManaged => {
                Box::new(models::terraform_service::TerraformService::<SCW>::new(
                    context,
                    self.long_id,
                    self.name,
                    self.kube_name,
                    self.action.to_service_action(),
                    KubernetesCpuResourceUnit::MilliCpu(self.cpu_request_in_milli),
                    KubernetesCpuResourceUnit::MilliCpu(self.cpu_limit_in_milli),
                    KubernetesMemoryResourceUnit::MebiByte(self.ram_request_in_mib),
                    KubernetesMemoryResourceUnit::MebiByte(self.ram_limit_in_mib),
                    self.gpu_request.map(KubernetesGpuResourceUnit),
                    self.gpu_limit.map(KubernetesGpuResourceUnit),
                    persistent_storage,
                    build,
                    tf_files_source_domain,
                    self.tf_var_file_paths,
                    self.tf_vars,
                    backend,
                    terraform_action,
                    Duration::from_secs(self.timeout_sec),
                    environment_variables_with_info,
                    self.extra_action_arguments,
                    self.advanced_settings,
                    |transmitter| context.get_event_details(transmitter),
                    annotations_groups,
                    labels_groups,
                    terraform_credentials_domain,
                )?)
            }
            Kind::Gke | Kind::GkeSelfManaged => Box::new(models::terraform_service::TerraformService::<GCP>::new(
                context,
                self.long_id,
                self.name,
                self.kube_name,
                self.action.to_service_action(),
                KubernetesCpuResourceUnit::MilliCpu(self.cpu_request_in_milli),
                KubernetesCpuResourceUnit::MilliCpu(self.cpu_limit_in_milli),
                KubernetesMemoryResourceUnit::MebiByte(self.ram_request_in_mib),
                KubernetesMemoryResourceUnit::MebiByte(self.ram_limit_in_mib),
                self.gpu_request.map(KubernetesGpuResourceUnit),
                self.gpu_limit.map(KubernetesGpuResourceUnit),
                persistent_storage,
                build,
                tf_files_source_domain,
                self.tf_var_file_paths,
                self.tf_vars,
                backend,
                terraform_action,
                Duration::from_secs(self.timeout_sec),
                environment_variables_with_info,
                self.extra_action_arguments,
                self.advanced_settings,
                |transmitter| context.get_event_details(transmitter),
                annotations_groups,
                labels_groups,
                terraform_credentials_domain,
            )?),
            Kind::Aks | Kind::AksSelfManaged => Box::new(models::terraform_service::TerraformService::<Azure>::new(
                context,
                self.long_id,
                self.name,
                self.kube_name,
                self.action.to_service_action(),
                KubernetesCpuResourceUnit::MilliCpu(self.cpu_request_in_milli),
                KubernetesCpuResourceUnit::MilliCpu(self.cpu_limit_in_milli),
                KubernetesMemoryResourceUnit::MebiByte(self.ram_request_in_mib),
                KubernetesMemoryResourceUnit::MebiByte(self.ram_limit_in_mib),
                self.gpu_request.map(KubernetesGpuResourceUnit),
                self.gpu_limit.map(KubernetesGpuResourceUnit),
                persistent_storage,
                build,
                tf_files_source_domain,
                self.tf_var_file_paths,
                self.tf_vars,
                backend,
                terraform_action,
                Duration::from_secs(self.timeout_sec),
                environment_variables_with_info,
                self.extra_action_arguments,
                self.advanced_settings,
                |transmitter| context.get_event_details(transmitter),
                annotations_groups,
                labels_groups,
                terraform_credentials_domain,
            )?),
            Kind::OnPremiseSelfManaged => Box::new(models::terraform_service::TerraformService::<OnPremise>::new(
                context,
                self.long_id,
                self.name,
                self.kube_name,
                self.action.to_service_action(),
                KubernetesCpuResourceUnit::MilliCpu(self.cpu_request_in_milli),
                KubernetesCpuResourceUnit::MilliCpu(self.cpu_limit_in_milli),
                KubernetesMemoryResourceUnit::MebiByte(self.ram_request_in_mib),
                KubernetesMemoryResourceUnit::MebiByte(self.ram_limit_in_mib),
                self.gpu_request.map(KubernetesGpuResourceUnit),
                self.gpu_limit.map(KubernetesGpuResourceUnit),
                persistent_storage,
                build,
                tf_files_source_domain,
                self.tf_var_file_paths,
                self.tf_vars,
                backend,
                terraform_action,
                Duration::from_secs(self.timeout_sec),
                environment_variables_with_info,
                self.extra_action_arguments,
                self.advanced_settings,
                |transmitter| context.get_event_details(transmitter),
                annotations_groups,
                labels_groups,
                terraform_credentials_domain,
            )?),
        };

        Ok(service)
    }

    fn get_terraform_files_source_domain(
        &self,
        ssh_keys: &[SshKey],
        qovery_api: Arc<dyn QoveryApi>,
        service_id: Uuid,
    ) -> models::terraform_service::TerraformFilesSource {
        match &self.tf_files_source {
            TerraformFilesSource::Git {
                git_url,
                git_credentials,
                commit_id,
                root_module_path,
            } => models::terraform_service::TerraformFilesSource::Git {
                git_url: git_url.clone(),
                get_credentials: if git_credentials.is_none() {
                    Box::new(|| Ok(None))
                } else {
                    Box::new(move || fetch_git_token(&*qovery_api, ServiceType::Terraform, &service_id).map(Some))
                },
                commit_id: commit_id.clone(),
                root_module_path: root_module_path.clone(),
                ssh_keys: ssh_keys.to_owned(),
            },
        }
    }

    fn get_terraform_credentials_domain(
        &self,
    ) -> Result<models::terraform_service::TerraformCredentials, TerraformServiceError> {
        let use_cluster_credentials = self
            .terraform_credentials
            .as_ref()
            .map(|creds| creds.use_cluster_credentials)
            .unwrap_or(false);

        Ok(models::terraform_service::TerraformCredentials {
            use_cluster_credentials,
        })
    }

    fn get_terraform_action(&self) -> Result<models::terraform_service::TerraformAction, TerraformServiceError> {
        let plan_execution_id =
            self.terraform_action
                .plan_execution_id
                .clone()
                .ok_or(TerraformServiceError::InvalidConfig(
                    "terraform_action plan_execution_id path is not defined".to_string(),
                ));

        let action = match self.terraform_action.command {
            TerraformActionCommand::PlanOnly => models::terraform_service::TerraformAction::TerraformPlanOnly {
                execution_id: plan_execution_id?,
            },
            TerraformActionCommand::PlanAndApply => models::terraform_service::TerraformAction::TerraformPlanAndApply,
            TerraformActionCommand::Destroy => models::terraform_service::TerraformAction::TerraformDestroy,
            TerraformActionCommand::ApplyFromPlan => {
                models::terraform_service::TerraformAction::TerraformApplyFromPlan {
                    execution_id: plan_execution_id?,
                }
            }
            TerraformActionCommand::ForceUnlockState => {
                models::terraform_service::TerraformAction::TerraformUnlockState
            }
            TerraformActionCommand::Init => models::terraform_service::TerraformAction::TerraformInit,
            TerraformActionCommand::Noop => models::terraform_service::TerraformAction::TerraformNoop,
        };

        Ok(action)
    }

    fn get_terraform_backend(
        &self,
        environment_kube_name: &str,
        environment_long_id: Uuid,
    ) -> Result<models::terraform_service::TerraformBackend, TerraformServiceError> {
        let configs = match self.backend.backend_type {
            TerraformBackendType::DefinedInTerraformFile => vec![],
            TerraformBackendType::Kubernetes => vec![
                models::terraform_service::TerraformBackendConfig::from_str(&format!(
                    r#"namespace="{environment_kube_name}""#
                ))
                .map_err(TerraformServiceError::InvalidConfig)?,
                models::terraform_service::TerraformBackendConfig::from_str(&format!(
                    r#"secret_suffix="{}""#,
                    self.long_id
                ))
                .map_err(TerraformServiceError::InvalidConfig)?,
                models::terraform_service::TerraformBackendConfig::from_str(
                    &format!(r#"labels={{"qovery.com/service-id": "{}", "qovery.com/service-type": "terraform-service", "qovery.com/environment-id": "{environment_long_id}" }}"#
                        , self.long_id),
                )
                .map_err(TerraformServiceError::InvalidConfig)?,
            ],
        };

        Ok(models::terraform_service::TerraformBackend {
            configs,
            kube_secret_name: format!("{}-backend-config", self.long_id),
        })
    }

    fn build_for_terraform_service(
        &self,
        ssh_keys: &[SshKey],
        qovery_api: Arc<dyn QoveryApi>,
        registry_url: &ContainerRegistryInfo,
        architectures: Vec<CpuArchitecture>,
        cluster_id: &QoveryIdentifier,
    ) -> Result<Build, TerraformServiceError> {
        let qovery_dockerfile = Some("Dockerfile.qovery".to_string());
        let (git_url, git_credentials, commit_id, dockerfile_path, dockerfile_content, root_module_path) =
            match &self.tf_files_source {
                TerraformFilesSource::Git {
                    git_url,
                    git_credentials,
                    commit_id,
                    root_module_path,
                } => (
                    git_url,
                    git_credentials,
                    commit_id,
                    &qovery_dockerfile,
                    self.get_docker_file(),
                    root_module_path,
                ),
            };

        // Use root_module_path as the Docker build context so only necessary files are copied
        let (root_path, dockerfile_path) = normalize_root_and_dockerfile_path(root_module_path, dockerfile_path);
        let mut disable_build_cache = false;

        let build_env_vars = self
            .environment_vars_with_infos
            .iter()
            .filter_map(|(k, variable_infos)| {
                // Remove special vars
                let v = String::from_utf8(
                    general_purpose::STANDARD
                        .decode(variable_infos.value.as_bytes())
                        .unwrap_or_default(),
                )
                .unwrap_or_default();
                if k == "QOVERY_DISABLE_BUILD_CACHE" && v.to_lowercase() == "true" {
                    disable_build_cache = true;
                    return None;
                }

                Some((k.clone(), v))
            })
            .collect::<BTreeMap<_, _>>();

        let extra_files_to_inject = self.build_extra_files(root_module_path)?;

        let mut build = Build {
            git_repository: GitRepository {
                url: git_url.clone(),
                get_credentials: if git_credentials.is_none() {
                    None
                } else {
                    let id = self.long_id;
                    Some(Box::new(move || fetch_git_token(&*qovery_api, ServiceType::Terraform, &id)))
                },
                ssh_keys: ssh_keys.to_vec(),
                commit_id: commit_id.clone(),
                dockerfile_path,
                dockerfile_content: Some(dockerfile_content),
                root_path: root_path.clone(),
                extra_files_to_inject,
                docker_target_build_stage: None,
            },
            image: self.to_image(commit_id.to_string(), registry_url, cluster_id, git_url.as_str()),
            environment_variables: build_env_vars,
            disable_cache: disable_build_cache,
            timeout: Duration::from_secs(self.advanced_settings.build_timeout_max_sec as u64),
            architectures,
            max_cpu_in_milli: self.advanced_settings.build_cpu_max_in_milli,
            max_ram_in_gib: self.advanced_settings.build_ram_max_in_gib,
            ephemeral_storage_in_gib: self.advanced_settings.build_ephemeral_storage_in_gib,
            registries: vec![],
        };

        build.compute_image_tag();

        Ok(build)
    }

    /// Generates the Dockerfile content for the Terraform/OpenTofu service.
    ///
    /// The returned Dockerfile contains a `{{custom_fragment}}` placeholder that will be
    /// replaced at build time with the content of `qovery-build-fragment.dockerfile` if
    /// present in the user's repository.
    fn get_docker_file(&self) -> String {
        let dockerfile = match &self.provider {
            TerraformProvider::Terraform => include_str!("resources/terraform.dockerfile"),
            TerraformProvider::OpenTofu => include_str!("resources/opentofu.dockerfile"),
        };

        dockerfile.replace("{{provider_version}}", &self.provider_version)
    }

    fn get_entry_point_sh(&self) -> String {
        let entry_point_sh = include_str!("resources/entrypoint.sh");
        match self.provider {
            TerraformProvider::Terraform => entry_point_sh.replace("{{terraform_command}}", "terraform"),
            TerraformProvider::OpenTofu => entry_point_sh.replace("{{terraform_command}}", "tofu"),
        }
    }

    fn get_backend_block(&self) -> Option<String> {
        match self.backend.backend_type {
            TerraformBackendType::DefinedInTerraformFile => None,
            TerraformBackendType::Kubernetes => Some(format!(
                r#"
terraform {{
  backend "{}" {{
  }}
}}"#,
                self.backend.backend_type.to_backend_block_name()
            )),
        }
    }

    fn build_extra_files(&self, root_module_path: &str) -> Result<Vec<GitRepositoryExtraFile>, TerraformServiceError> {
        // Place entrypoint.sh in root_module_path so it ends up at /data/entrypoint.sh in the Docker image
        let (_, entry_point_file_path) =
            normalize_root_and_dockerfile_path(root_module_path, &Some("entrypoint.sh".to_string()));

        let mut extra_files = vec![GitRepositoryExtraFile {
            path: entry_point_file_path
                .ok_or_else(|| TerraformServiceError::InvalidConfig("entrypoint.sh path is not defined".to_string()))?,
            content: self.get_entry_point_sh(),
        }];

        if let Some(backend_block) = self.get_backend_block() {
            let (_, backend_file_path) =
                normalize_root_and_dockerfile_path(root_module_path, &Some("backend.tf".to_string()));
            let backend_file_path = backend_file_path
                .ok_or_else(|| TerraformServiceError::InvalidConfig("Backend path is not defined".to_string()))?;

            extra_files.push(GitRepositoryExtraFile {
                path: backend_file_path,
                content: backend_block.to_string(),
            });
        }

        Ok(extra_files)
    }

    fn to_image(
        &self,
        commit_id: String,
        cr_info: &ContainerRegistryInfo,
        cluster_id: &QoveryIdentifier,
        git_url: &str,
    ) -> Image {
        let repository_name = cr_info.get_repository_name(&self.name);
        let image_name = match self.shared_image_feature_enabled {
            true => cr_info.get_shared_image_name(cluster_id, sanitized_git_url(git_url)),
            false => cr_info.get_image_name(&self.long_id.to_string()),
        };
        Image {
            service_id: to_short_id(&self.long_id),
            service_long_id: self.long_id,
            service_name: self.name.clone(),
            name: image_name.to_string(),
            tag: "".to_string(), // It needs to be computed after creation
            commit_id,
            registry_name: cr_info.registry_name.clone(),
            registry_url: cr_info.get_registry_endpoint(Some(cluster_id.qovery_resource_name())),
            registry_insecure: cr_info.insecure_registry,
            registry_docker_json_config: cr_info.get_registry_docker_json_config(DockerRegistryInfo {
                registry_name: Some(cr_info.registry_name.to_string()),
                repository_name: Some(repository_name.to_string()),
                image_name: Some(image_name.to_string()),
            }),
            repository_name: cr_info.get_repository_name(&self.long_id.to_string()),
            shared_repository_name: cr_info.get_shared_repository_name(cluster_id, sanitized_git_url(git_url)),
            shared_image_feature_enabled: self.shared_image_feature_enabled,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_terraform_service(kube_name: &str) -> TerraformService {
        TerraformService {
            long_id: Uuid::new_v4(),
            name: "test-service".to_string(),
            kube_name: kube_name.to_string(),
            action: Action::Create,
            cpu_request_in_milli: 100,
            cpu_limit_in_milli: 200,
            ram_request_in_mib: 128,
            ram_limit_in_mib: 256,
            gpu_request: None,
            gpu_limit: None,
            persistent_storage: PersistentStorage {
                storage_class: "standard".to_string(),
                size_in_gib: 1,
            },
            tf_files_source: TerraformFilesSource::Git {
                git_url: Url::parse("https://github.com/test/repo").unwrap(),
                git_credentials: None,
                commit_id: "abc123".to_string(),
                root_module_path: ".".to_string(),
            },
            tf_var_file_paths: vec![],
            tf_vars: vec![],
            provider: TerraformProvider::Terraform,
            provider_version: "1.0.0".to_string(),
            backend: TerraformBackend {
                backend_type: TerraformBackendType::Kubernetes,
            },
            terraform_action: TerraformAction {
                command: TerraformActionCommand::PlanAndApply,
                plan_execution_id: None,
            },
            timeout_sec: 600,
            environment_vars_with_infos: BTreeMap::new(),
            advanced_settings: TerraformServiceAdvancedSettings::default(),
            annotations_group_ids: BTreeSet::new(),
            labels_group_ids: BTreeSet::new(),
            shared_image_feature_enabled: false,
            terraform_credentials: None,
            extra_action_arguments: BTreeMap::new(),
        }
    }

    #[test]
    fn test_backend_config_secret_name_is_unique_per_service() {
        // Given two terraform services with different long_ids
        let service1 = create_test_terraform_service("terraform-abc123-service1");
        let service2 = create_test_terraform_service("terraform-def456-service2");
        let env_kube_name = "test-environment";
        let env_long_id = Uuid::new_v4();

        // When we get the terraform backend for each
        let backend1 = service1.get_terraform_backend(env_kube_name, env_long_id).unwrap();
        let backend2 = service2.get_terraform_backend(env_kube_name, env_long_id).unwrap();

        // Then each backend should have a unique secret name based on the service's long_id (UUID)
        assert_eq!(
            backend1.kube_secret_name,
            format!("{}-backend-config", service1.long_id),
            "Backend config secret name should include the service's long_id"
        );
        assert_eq!(
            backend2.kube_secret_name,
            format!("{}-backend-config", service2.long_id),
            "Backend config secret name should include the service's long_id"
        );

        // And the secret names should be different
        assert_ne!(
            backend1.kube_secret_name, backend2.kube_secret_name,
            "Two different services should have different backend config secret names"
        );
    }

    #[test]
    fn test_backend_config_secret_name_format() {
        let service = create_test_terraform_service("my-terraform-service");
        let env_kube_name = "production-env";
        let env_long_id = Uuid::new_v4();

        let backend = service.get_terraform_backend(env_kube_name, env_long_id).unwrap();

        // Secret name should follow the pattern: {long_id}-backend-config
        assert!(
            backend.kube_secret_name.ends_with("-backend-config"),
            "Secret name should end with '-backend-config'"
        );
        assert!(
            backend.kube_secret_name.starts_with(&service.long_id.to_string()),
            "Secret name should start with the service's long_id (UUID)"
        );
    }

    #[test]
    fn test_backend_config_for_defined_in_terraform_file_type() {
        let mut service = create_test_terraform_service("terraform-service");
        service.backend.backend_type = TerraformBackendType::DefinedInTerraformFile;
        let env_kube_name = "test-env";
        let env_long_id = Uuid::new_v4();

        let backend = service.get_terraform_backend(env_kube_name, env_long_id).unwrap();

        // Even for DefinedInTerraformFile type, the secret name should use the service UUID
        assert_eq!(
            backend.kube_secret_name,
            format!("{}-backend-config", service.long_id),
            "Secret name should use service UUID even when backend type is DefinedInTerraformFile"
        );
        // And configs should be empty for this backend type
        assert!(backend.configs.is_empty());
    }

    #[test]
    fn test_get_docker_file_contains_custom_fragment_placeholder() {
        let service = create_test_terraform_service("terraform-service");

        let dockerfile = service.get_docker_file();

        // Should contain the placeholder for build-time injection
        assert!(dockerfile.contains("{{custom_fragment}}"));
        // Should have provider version replaced
        assert!(dockerfile.contains("1.0.0"));
        assert!(!dockerfile.contains("{{provider_version}}"));
    }

    #[test]
    fn test_get_docker_file_opentofu_contains_custom_fragment_placeholder() {
        let mut service = create_test_terraform_service("opentofu-service");
        service.provider = TerraformProvider::OpenTofu;

        let dockerfile = service.get_docker_file();

        // Should contain the placeholder for build-time injection
        assert!(dockerfile.contains("{{custom_fragment}}"));
        // Should have provider version replaced
        assert!(dockerfile.contains("1.0.0"));
        assert!(!dockerfile.contains("{{provider_version}}"));
        // Should be OpenTofu image
        assert!(dockerfile.contains("opentofu"));
    }
}

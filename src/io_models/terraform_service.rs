use crate::engine_task::qovery_api::QoveryApi;
use crate::environment::models;
use crate::environment::models::terraform_service::{TerraformServiceError, TerraformServiceTrait};
use crate::environment::models::types::{OnPremise, VersionsNumber, AWS, GCP, SCW};
use crate::infrastructure::models::build_platform::{Build, GitRepository, GitRepositoryExtraFile, Image, SshKey};
use crate::infrastructure::models::cloud_provider::service::ServiceType;
use crate::infrastructure::models::cloud_provider::CloudProvider;
use crate::infrastructure::models::container_registry::{ContainerRegistry, ContainerRegistryInfo};
use crate::infrastructure::models::kubernetes::{Kind, Kubernetes};
use crate::io_models::annotations_group::AnnotationsGroup;
use crate::io_models::application::GitCredentials;
use crate::io_models::context::Context;
use crate::io_models::labels_group::LabelsGroup;
use crate::io_models::models::CpuArchitecture;
use crate::io_models::variable_utils::{default_environment_vars_with_info, VariableInfo};
use crate::io_models::{
    fetch_git_token, normalize_root_and_dockerfile_path, sanitized_git_url, ssh_keys_from_env_vars, Action,
    QoveryIdentifier,
};
use crate::utilities::to_short_id;
use base64::engine::general_purpose;
use base64::Engine;
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
#[serde(rename_all = "snake_case")]
pub enum TerraformProvider {
    Terraform,
    // OpenTofu
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
pub struct TerraformBackend {
    pub backend_type: TerraformBackendType,
    pub block: String,
    pub configs: Vec<TerraformBackendConfig>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TerraformBackendType {
    DefinedInTerraformFile,
    Kubernetes,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash, Default)]
pub struct TerraformBackendConfig {
    key: String,
    value: String,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct TerraformService {
    pub long_id: Uuid,
    pub name: String,
    pub kube_name: String,
    pub action: Action,
    pub tf_files_source: TerraformFilesSource,
    pub provider: TerraformProvider,
    pub provider_version: String,
    pub backend: TerraformBackend,
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
}

impl TerraformService {
    pub fn to_terraform_service_domain(
        self,
        context: &Context,
        cloud_provider: &dyn CloudProvider,
        default_container_registry: &dyn ContainerRegistry,
        cluster: &dyn Kubernetes,
        annotations_group: &BTreeMap<Uuid, AnnotationsGroup>,
        labels_group: &BTreeMap<Uuid, LabelsGroup>,
    ) -> Result<Box<dyn TerraformServiceTrait>, TerraformServiceError> {
        // Get passphrase and public key if provided by the user
        let ssh_keys = ssh_keys_from_env_vars(&self.environment_vars_with_infos);
        let environment_variables_with_info: HashMap<String, VariableInfo> = self
            .environment_vars_with_infos
            .clone()
            .into_iter()
            .map(|(k, mut v)| {
                v.value =
                    String::from_utf8_lossy(&general_purpose::STANDARD.decode(v.value).unwrap_or_default()).to_string();
                (k, v)
            })
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

        let build = self.get_build(
            &ssh_keys,
            context.qovery_api.clone(),
            default_container_registry.registry_info(),
            cluster.cpu_architectures(),
            &QoveryIdentifier::new(*cluster.long_id()),
        )?;

        let tf_files_source_domain =
            self.get_terraform_files_source_domain(&ssh_keys, context.qovery_api.clone(), self.long_id);

        let provider = self.get_terraform_provider();
        let provider_version = VersionsNumber::from_str(self.provider_version.as_str()).map_err(|_| {
            TerraformServiceError::InvalidConfig(format!("Bad version number: {}", self.provider_version))
        })?;
        let backend = self.get_terraform_backend();

        let service: Box<dyn TerraformServiceTrait> = match cloud_provider.kubernetes_kind() {
            Kind::Eks | Kind::EksSelfManaged => Box::new(models::terraform_service::TerraformService::<AWS>::new(
                context,
                self.long_id,
                self.name,
                self.kube_name,
                self.action.to_service_action(),
                build,
                tf_files_source_domain,
                provider,
                provider_version,
                backend,
                Duration::from_secs(self.timeout_sec),
                environment_variables_with_info,
                self.advanced_settings,
                |transmitter| context.get_event_details(transmitter),
                annotations_groups,
                labels_groups,
            )?),
            Kind::ScwKapsule | Kind::ScwSelfManaged => {
                Box::new(models::terraform_service::TerraformService::<SCW>::new(
                    context,
                    self.long_id,
                    self.name,
                    self.kube_name,
                    self.action.to_service_action(),
                    build,
                    tf_files_source_domain,
                    provider,
                    provider_version,
                    backend,
                    Duration::from_secs(self.timeout_sec),
                    environment_variables_with_info,
                    self.advanced_settings,
                    |transmitter| context.get_event_details(transmitter),
                    annotations_groups,
                    labels_groups,
                )?)
            }
            Kind::Gke | Kind::GkeSelfManaged => Box::new(models::terraform_service::TerraformService::<GCP>::new(
                context,
                self.long_id,
                self.name,
                self.kube_name,
                self.action.to_service_action(),
                build,
                tf_files_source_domain,
                provider,
                provider_version,
                backend,
                Duration::from_secs(self.timeout_sec),
                environment_variables_with_info,
                self.advanced_settings,
                |transmitter| context.get_event_details(transmitter),
                annotations_groups,
                labels_groups,
            )?),
            Kind::OnPremiseSelfManaged => Box::new(models::terraform_service::TerraformService::<OnPremise>::new(
                context,
                self.long_id,
                self.name,
                self.kube_name,
                self.action.to_service_action(),
                build,
                tf_files_source_domain,
                provider,
                provider_version,
                backend,
                Duration::from_secs(self.timeout_sec),
                environment_variables_with_info,
                self.advanced_settings,
                |transmitter| context.get_event_details(transmitter),
                annotations_groups,
                labels_groups,
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

    fn get_terraform_provider(&self) -> models::terraform_service::TerraformProvider {
        match self.provider {
            TerraformProvider::Terraform => models::terraform_service::TerraformProvider::Terraform,
        }
    }

    fn get_terraform_backend(&self) -> models::terraform_service::TerraformBackend {
        let backend = &self.backend;
        models::terraform_service::TerraformBackend {
            backend_type: match backend.backend_type {
                TerraformBackendType::DefinedInTerraformFile => {
                    models::terraform_service::TerraformBackendType::DefinedInTerraformFile
                }
                TerraformBackendType::Kubernetes => models::terraform_service::TerraformBackendType::Kubernetes,
            },
            block: models::terraform_service::TerraformBackendBlock::from(&backend.block),
            configs: backend
                .configs
                .iter()
                .map(|config| models::terraform_service::TerraformBackendConfig {
                    key: config.key.clone(),
                    value: config.value.clone(),
                })
                .collect(),
        }
    }

    fn get_build(
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

        // Convert our root module path to a relative path to be able to append them correctly
        let (root_path, dockerfile_path) = normalize_root_and_dockerfile_path(root_module_path, dockerfile_path);
        let (_, backend_file_path) =
            normalize_root_and_dockerfile_path(root_module_path, &Some("backend_qovery.tf".to_string()));

        let mut disable_build_cache = false;
        let mut build = Build {
            git_repository: GitRepository {
                url: git_url.clone(),
                get_credentials: if git_credentials.is_none() {
                    None
                } else {
                    let id = self.long_id;
                    Some(Box::new(move || fetch_git_token(&*qovery_api, ServiceType::Job, &id)))
                },
                ssh_keys: ssh_keys.to_vec(),
                commit_id: commit_id.clone(),
                dockerfile_path,
                dockerfile_content: Some(dockerfile_content),
                root_path: root_path.clone(),
                extra_files_to_inject: vec![GitRepositoryExtraFile {
                    path: backend_file_path.ok_or(TerraformServiceError::InvalidConfig(
                        "Backend path path is not defined".to_string(),
                    ))?,
                    content: self.backend.block.clone(),
                }],
            },
            image: self.to_image(commit_id.to_string(), registry_url, cluster_id, git_url.as_str()),
            environment_variables: self
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
                .collect::<BTreeMap<_, _>>(),
            disable_cache: disable_build_cache,
            timeout: Duration::from_secs(self.advanced_settings.build_timeout_max_sec as u64),
            architectures,
            max_cpu_in_milli: self.advanced_settings.build_cpu_max_in_milli,
            max_ram_in_gib: self.advanced_settings.build_ram_max_in_gib,
            ephemeral_storage_in_gib: self.advanced_settings.build_ephemeral_storage_in_gib,
            registries: vec![], // TODO TF
        };

        // TODO TF: the image tag must be changed:
        // if the Terraform provider changed,
        // if the Terraform provider Version changed,
        // if the files added into the Docker imaged changed (backend, entrypoint.sh)
        build.compute_image_tag();

        Ok(build)
    }

    fn get_docker_file(&self) -> String {
        // TODO TF remove from here, use a mirror of  hashicorp/terraform, customize version, path, parameter of terraform init,
        format!(
            r#"FROM hashicorp/terraform:{}

WORKDIR /app
COPY . .

RUN ls # TODO TF to be removed
                    "#,
            self.provider_version
        )
    }

    fn to_image(
        &self,
        commit_id: String,
        cr_info: &ContainerRegistryInfo,
        cluster_id: &QoveryIdentifier,
        git_url: &str,
    ) -> Image {
        Image {
            service_id: to_short_id(&self.long_id),
            service_long_id: self.long_id,
            service_name: self.name.clone(),
            name: match self.shared_image_feature_enabled {
                true => cr_info.get_shared_image_name(cluster_id, sanitized_git_url(git_url)),
                false => cr_info.get_image_name(&self.long_id.to_string()),
            },
            tag: "".to_string(), // It needs to be computed after creation
            commit_id,
            registry_name: cr_info.registry_name.clone(),
            registry_url: cr_info.endpoint.clone(),
            registry_insecure: cr_info.insecure_registry,
            registry_docker_json_config: cr_info.registry_docker_json_config.clone(),
            repository_name: cr_info.get_repository_name(&self.long_id.to_string()),
            shared_repository_name: cr_info.get_shared_repository_name(cluster_id, sanitized_git_url(git_url)),
            shared_image_feature_enabled: self.shared_image_feature_enabled,
        }
    }
}

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
use crate::io_models::models::{CpuArchitecture, KubernetesMemoryResourceUnit};
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
use std::path::PathBuf;
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
    pub configs: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "snake_case")]
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
pub enum TerraformActionCommand {
    PlanOnly,
    PlanAndApply,
    ApplyFromPlan,
    Destroy,
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
pub struct TerraformService {
    pub long_id: Uuid,
    pub name: String,
    pub kube_name: String,
    pub action: Action,
    pub cpu_request_in_milli: u32,
    pub cpu_limit_in_milli: u32,
    pub ram_request_in_mib: u32,
    pub ram_limit_in_mib: u32,
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
}

impl TerraformService {
    pub fn to_terraform_service_domain(
        self,
        context: &Context,
        cloud_provider: &dyn CloudProvider,
        default_container_registry: &dyn InteractWithRegistry,
        cluster: &dyn Kubernetes,
        environment_kube_name: &str,
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

        let build = self.build_for_terraform_service(
            &ssh_keys,
            context.qovery_api.clone(),
            default_container_registry.registry_info(),
            cluster.cpu_architectures(),
            &QoveryIdentifier::new(*cluster.long_id()),
        )?;

        let tf_files_source_domain =
            self.get_terraform_files_source_domain(&ssh_keys, context.qovery_api.clone(), self.long_id);

        let backend = self.get_terraform_backend(environment_kube_name)?;

        let terraform_action = self.get_terraform_action()?;

        let persistent_storage = models::terraform_service::PersistentStorage {
            storage_class: self.persistent_storage.storage_class,
            size_in_gib: KubernetesMemoryResourceUnit::GibiByte(self.persistent_storage.size_in_gib),
        };

        let root_module_path = match self.tf_files_source {
            TerraformFilesSource::Git { root_module_path, .. } => PathBuf::from(root_module_path),
        };

        let service: Box<dyn TerraformServiceTrait> = match cloud_provider.kubernetes_kind() {
            Kind::Eks | Kind::EksSelfManaged => Box::new(models::terraform_service::TerraformService::<AWS>::new(
                context,
                self.long_id,
                self.name,
                self.kube_name,
                self.action.to_service_action(),
                self.cpu_request_in_milli,
                self.cpu_limit_in_milli,
                self.ram_request_in_mib,
                self.ram_limit_in_mib,
                persistent_storage,
                build,
                root_module_path,
                tf_files_source_domain,
                self.tf_var_file_paths,
                self.tf_vars,
                backend,
                terraform_action,
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
                    self.cpu_request_in_milli,
                    self.cpu_limit_in_milli,
                    self.ram_request_in_mib,
                    self.ram_limit_in_mib,
                    persistent_storage,
                    build,
                    root_module_path,
                    tf_files_source_domain,
                    self.tf_var_file_paths,
                    self.tf_vars,
                    backend,
                    terraform_action,
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
                self.cpu_request_in_milli,
                self.cpu_limit_in_milli,
                self.ram_request_in_mib,
                self.ram_limit_in_mib,
                persistent_storage,
                build,
                root_module_path,
                tf_files_source_domain,
                self.tf_var_file_paths,
                self.tf_vars,
                backend,
                terraform_action,
                Duration::from_secs(self.timeout_sec),
                environment_variables_with_info,
                self.advanced_settings,
                |transmitter| context.get_event_details(transmitter),
                annotations_groups,
                labels_groups,
            )?),
            Kind::Aks | Kind::AksSelfManaged => Box::new(models::terraform_service::TerraformService::<Azure>::new(
                context,
                self.long_id,
                self.name,
                self.kube_name,
                self.action.to_service_action(),
                self.cpu_request_in_milli,
                self.cpu_limit_in_milli,
                self.ram_request_in_mib,
                self.ram_limit_in_mib,
                persistent_storage,
                build,
                root_module_path,
                tf_files_source_domain,
                self.tf_var_file_paths,
                self.tf_vars,
                backend,
                terraform_action,
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
                self.cpu_request_in_milli,
                self.cpu_limit_in_milli,
                self.ram_request_in_mib,
                self.ram_limit_in_mib,
                persistent_storage,
                build,
                root_module_path,
                tf_files_source_domain,
                self.tf_var_file_paths,
                self.tf_vars,
                backend,
                terraform_action,
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
        };

        Ok(action)
    }

    fn get_terraform_backend(
        &self,
        environment_kube_name: &str,
    ) -> Result<models::terraform_service::TerraformBackend, TerraformServiceError> {
        let configs = match self.backend.backend_type {
            TerraformBackendType::DefinedInTerraformFile => self
                .backend
                .configs
                .iter()
                .map(|config| {
                    models::terraform_service::TerraformBackendConfig::from_str(config)
                        .map_err(TerraformServiceError::InvalidConfig)
                })
                .collect::<Result<Vec<_>, _>>()?,
            TerraformBackendType::Kubernetes => vec![
                models::terraform_service::TerraformBackendConfig::from_str(&format!(
                    "namespace=\"{}\"",
                    environment_kube_name
                ))
                .map_err(TerraformServiceError::InvalidConfig)?,
                models::terraform_service::TerraformBackendConfig::from_str(&format!(
                    "secret_suffix=\"{}\"",
                    self.long_id
                ))
                .map_err(TerraformServiceError::InvalidConfig)?,
                models::terraform_service::TerraformBackendConfig::from_str(
                    &format!("labels={{\"qovery.com/service-id\": \"{}\", \"qovery.com/service-type\": \"terraform-service\", \"qovery.com/environment-id\": \"{}\" }}", self.long_id, environment_kube_name),
                )
                .map_err(TerraformServiceError::InvalidConfig)?,
            ],
        };

        Ok(models::terraform_service::TerraformBackend {
            configs,
            kube_secret_name: "backend-config".to_string(),
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

        // Convert our root module path to a relative path to be able to append them correctly
        let (root_path, dockerfile_path) = normalize_root_and_dockerfile_path("/", dockerfile_path);
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

    fn get_docker_file(&self) -> String {
        // TODO TF remove from here, use a mirror of  hashicorp/terraform, customize version, path, parameter of terraform init,
        format!(
            r#"FROM hashicorp/terraform:{}
RUN <<EOF
set -e
apk update
apk add dumb-init rsync
adduser -D -u 1000 app
mkdir /data
chown -R app:app /data
EOF

WORKDIR /data
COPY . .
RUN ls

RUN chmod +x entrypoint.sh
USER app

ENTRYPOINT ["/usr/bin/dumb-init", "-v", "--", "/bin/sh", "/data/entrypoint.sh"]
                    "#,
            self.provider_version
        )
    }

    fn get_entry_point_sh(&self) -> String {
        // TODO TF remove from here
        r#"# entrypoint.sh
#!/bin/bash
set -x

echo "Starting entrypoint.sh"

ROOT_MODULE_PATH=$1
CMD=$2
PLAN_NAME=$3
shift 3
set -e

mkdir -p /persistent-volume/terraform-work
mkdir -p /persistent-volume/terraform-plan-output

rsync -av --delete \
          --exclude='entrypoint.sh' \
          --exclude='Dockerfile.qovery' \
          --exclude='.terraform' \
          --exclude='.terraform.lock.hcl' \
          --exclude='.-tf.plan' \
          /data/ /persistent-volume/terraform-work

cd /persistent-volume/terraform-work/$ROOT_MODULE_PATH

case "$CMD" in
    "apply")
        terraform init -backend-config="/backend-config/config"
        terraform validate -no-tests
        terraform apply -input=false -auto-approve "$@"
        terraform output -json > /qovery-output/qovery-output.json
        ;;
    "plan_only")
        rm -rf /persistent-volume/terraform-plan-output/*
        terraform init -backend-config="/backend-config/config"
        terraform validate -no-tests
        terraform plan -input=false -out=/persistent-volume/terraform-plan-output/${PLAN_NAME}-tf.plan "$@"
        ;;
    "apply_from_plan")
        terraform init -backend-config="/backend-config/config"
        terraform validate -no-tests
        terraform apply -input=false /persistent-volume/terraform-plan-output/${PLAN_NAME}-tf.plan
        terraform output -json > /qovery-output/qovery-output.json
        ;;
    "destroy")
        terraform destroy -auto-approve -input=false "$@"
        ;;
    *)
        echo "Command not handled by entrypoint.sh: '\$CMD'"
        exit 1
        ;;
esac
            "#
        .to_string()
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
        let (_, backend_file_path) =
            normalize_root_and_dockerfile_path(root_module_path, &Some("backend_qovery.tf".to_string()));
        let (_, entry_point_file_path) = normalize_root_and_dockerfile_path("/", &Some("entrypoint.sh".to_string()));

        let mut extra_files = vec![GitRepositoryExtraFile {
            path: entry_point_file_path
                .ok_or_else(|| TerraformServiceError::InvalidConfig("entrypoint.sh path is not defined".to_string()))?,
            content: self.get_entry_point_sh(),
        }];

        if let Some(backend_block) = self.get_backend_block() {
            extra_files.push(GitRepositoryExtraFile {
                path: backend_file_path
                    .ok_or_else(|| TerraformServiceError::InvalidConfig("Backend path is not defined".to_string()))?,
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

use k8s_openapi::api::core::v1::{Namespace, Secret, Service};
use kube::api::{ListParams, ObjectMeta, Patch, PatchParams, PostParams};
use kube::core::ObjectList;
use kube::{Api, Error};
use retry::delay::{Fibonacci, Fixed};
use retry::OperationResult;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::any::Any;
use std::collections::{BTreeMap, HashMap};
use std::fmt::{Display, Formatter};
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::mpsc::TryRecvError;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;
use strum_macros::EnumIter;
use tracing::Span;
use uuid::Uuid;

use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::io::ClusterAdvancedSettings;
use crate::cloud_provider::models::{CpuArchitecture, CpuLimits, InstanceEc2, NodeGroups};
use crate::cloud_provider::service::Action;
use crate::cloud_provider::CloudProvider;
use crate::cloud_provider::Kind as CloudProviderKind;
use crate::cmd::kubectl::{kubectl_delete_apiservice, kubectl_delete_completed_jobs};
use crate::cmd::kubectl::{
    kubectl_delete_objects_in_all_namespaces, kubectl_exec_count_all_objects, kubectl_exec_delete_pod,
    kubectl_exec_get_node, kubectl_exec_version, kubectl_get_crash_looping_pods, kubernetes_get_all_pdbs,
};
use crate::cmd::structs::KubernetesNodeCondition;
use crate::dns_provider::DnsProvider;
use crate::errors::{CommandError, EngineError, ErrorMessageVerbosity};
use crate::events::Stage::Infrastructure;
use crate::events::{EngineEvent, EventDetails, EventMessage, InfrastructureStep, Stage, Transmitter};
use crate::fs::{delete_file_if_exists, workspace_directory};
use crate::io_models::context::Context;
use crate::io_models::engine_request::{ChartValuesOverrideName, ChartValuesOverrideValues};
use crate::io_models::QoveryIdentifier;
use crate::logger::Logger;
use crate::metrics_registry::MetricsRegistry;
use crate::models::types::VersionsNumber;
use crate::object_storage::ObjectStorage;
use crate::runtime::block_on;
use crate::services::kube_client::QubeClient;
use crate::unit_conversion::{any_to_mi, cpu_string_to_float};
use crate::utilities::create_kube_client;

use super::models::NodeGroupsWithDesiredState;
use super::vault::ClusterSecrets;

pub trait ProviderOptions {}

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum KubernetesError {
    /// Triggered if an addon version is not supporting the given kubernetes version
    #[error("Addon `{addon}` doesn't support kubernetes version `{kubernetes_version}`.")]
    AddonUnSupportedKubernetesVersion {
        kubernetes_version: String,
        addon: KubernetesAddon,
    },
}

impl KubernetesError {
    /// Returns safe Kubernetes error message part (not full error message).
    pub fn to_safe_message(&self) -> String {
        match self {
            KubernetesError::AddonUnSupportedKubernetesVersion {
                kubernetes_version,
                addon,
            } => format!("Addon `{addon}` doesn't support kubernetes version `{kubernetes_version}`."),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum KubernetesAddon {
    Cni,
    EbsCsi,
}

impl Display for KubernetesAddon {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            KubernetesAddon::Cni => "cni",
            KubernetesAddon::EbsCsi => "ebs-csi",
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, EnumIter)]
pub enum KubernetesVersion {
    V1_23 {
        prefix: Option<Arc<str>>,
        patch: Option<u8>,
        suffix: Option<Arc<str>>,
    },
    V1_24 {
        prefix: Option<Arc<str>>,
        patch: Option<u8>,
        suffix: Option<Arc<str>>,
    },
    V1_25 {
        prefix: Option<Arc<str>>,
        patch: Option<u8>,
        suffix: Option<Arc<str>>,
    },
    V1_26 {
        prefix: Option<Arc<str>>,
        patch: Option<u8>,
        suffix: Option<Arc<str>>,
    },
    V1_27 {
        prefix: Option<Arc<str>>,
        patch: Option<u8>,
        suffix: Option<Arc<str>>,
    },
    V1_28 {
        prefix: Option<Arc<str>>,
        patch: Option<u8>,
        suffix: Option<Arc<str>>,
    },
}

impl KubernetesVersion {
    pub fn prefix(&self) -> &Option<Arc<str>> {
        match &self {
            KubernetesVersion::V1_23 { prefix, .. } => prefix,
            KubernetesVersion::V1_24 { prefix, .. } => prefix,
            KubernetesVersion::V1_25 { prefix, .. } => prefix,
            KubernetesVersion::V1_26 { prefix, .. } => prefix,
            KubernetesVersion::V1_27 { prefix, .. } => prefix,
            KubernetesVersion::V1_28 { prefix, .. } => prefix,
        }
    }

    pub fn major(&self) -> u8 {
        match &self {
            KubernetesVersion::V1_23 { .. } => 1,
            KubernetesVersion::V1_24 { .. } => 1,
            KubernetesVersion::V1_25 { .. } => 1,
            KubernetesVersion::V1_26 { .. } => 1,
            KubernetesVersion::V1_27 { .. } => 1,
            KubernetesVersion::V1_28 { .. } => 1,
        }
    }

    pub fn minor(&self) -> u8 {
        match &self {
            KubernetesVersion::V1_23 { .. } => 23,
            KubernetesVersion::V1_24 { .. } => 24,
            KubernetesVersion::V1_25 { .. } => 25,
            KubernetesVersion::V1_26 { .. } => 26,
            KubernetesVersion::V1_27 { .. } => 27,
            KubernetesVersion::V1_28 { .. } => 28,
        }
    }

    pub fn patch(&self) -> &Option<u8> {
        match &self {
            KubernetesVersion::V1_23 { patch, .. } => patch,
            KubernetesVersion::V1_24 { patch, .. } => patch,
            KubernetesVersion::V1_25 { patch, .. } => patch,
            KubernetesVersion::V1_26 { patch, .. } => patch,
            KubernetesVersion::V1_27 { patch, .. } => patch,
            KubernetesVersion::V1_28 { patch, .. } => patch,
        }
    }

    pub fn suffix(&self) -> &Option<Arc<str>> {
        match &self {
            KubernetesVersion::V1_23 { suffix, .. } => suffix,
            KubernetesVersion::V1_24 { suffix, .. } => suffix,
            KubernetesVersion::V1_25 { suffix, .. } => suffix,
            KubernetesVersion::V1_26 { suffix, .. } => suffix,
            KubernetesVersion::V1_27 { suffix, .. } => suffix,
            KubernetesVersion::V1_28 { suffix, .. } => suffix,
        }
    }

    pub fn next_version(&self) -> Option<Self> {
        match self {
            KubernetesVersion::V1_23 { .. } => Some(KubernetesVersion::V1_24 {
                prefix: None,
                patch: None,
                suffix: None,
            }),
            KubernetesVersion::V1_24 { .. } => Some(KubernetesVersion::V1_25 {
                prefix: None,
                patch: None,
                suffix: None,
            }),
            KubernetesVersion::V1_25 { .. } => Some(KubernetesVersion::V1_26 {
                prefix: None,
                patch: None,
                suffix: None,
            }),
            KubernetesVersion::V1_26 { .. } => Some(KubernetesVersion::V1_27 {
                prefix: None,
                patch: None,
                suffix: None,
            }),
            KubernetesVersion::V1_27 { .. } => Some(KubernetesVersion::V1_28 {
                prefix: None,
                patch: None,
                suffix: None,
            }),
            KubernetesVersion::V1_28 { .. } => None,
        }
    }
}

impl Display for KubernetesVersion {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let prefix = match self.prefix() {
            None => "",
            Some(p) => p,
        };

        let suffix = match self.suffix() {
            None => "",
            Some(s) => s,
        };

        match self.patch() {
            None => f.write_fmt(format_args!("{}{}.{}{}", prefix, self.major(), self.minor(), suffix)),
            Some(patch) => f.write_fmt(format_args!("{}{}.{}.{}{}", prefix, self.major(), self.minor(), patch, suffix)),
        }
    }
}

impl From<KubernetesVersion> for VersionsNumber {
    fn from(val: KubernetesVersion) -> Self {
        VersionsNumber {
            major: val.major().to_string(),
            minor: Some(val.minor().to_string()),
            patch: match val.patch().is_some() {
                true => Some(val.patch().as_ref().unwrap_or(&0).to_string()),
                false => None,
            },
            suffix: match val.suffix().is_some() {
                true => Some(val.suffix().as_ref().unwrap_or(&Arc::from("")).to_string()),
                false => None,
            },
        }
    }
}

impl FromStr for KubernetesVersion {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "1.23" => Ok(KubernetesVersion::V1_23 {
                prefix: None,
                patch: None,
                suffix: None,
            }),
            "1.24" => Ok(KubernetesVersion::V1_24 {
                prefix: None,
                patch: None,
                suffix: None,
            }),
            "1.25" => Ok(KubernetesVersion::V1_25 {
                prefix: None,
                patch: None,
                suffix: None,
            }),
            "1.26" => Ok(KubernetesVersion::V1_26 {
                prefix: None,
                patch: None,
                suffix: None,
            }),
            "1.27" => Ok(KubernetesVersion::V1_27 {
                prefix: None,
                patch: None,
                suffix: None,
            }),
            "1.28" => Ok(KubernetesVersion::V1_28 {
                prefix: None,
                patch: None,
                suffix: None,
            }),
            // EC2 specifics
            "v1.23.16+k3s1" => Ok(KubernetesVersion::V1_23 {
                prefix: Some(Arc::from("v")),
                patch: Some(16),
                suffix: Some(Arc::from("+k3s1")),
            }),
            "v1.24.14+k3s1" => Ok(KubernetesVersion::V1_24 {
                prefix: Some(Arc::from("v")),
                patch: Some(14),
                suffix: Some(Arc::from("+k3s1")),
            }),
            "v1.25.11+k3s1" => Ok(KubernetesVersion::V1_25 {
                prefix: Some(Arc::from("v")),
                patch: Some(11),
                suffix: Some(Arc::from("+k3s1")),
            }),
            "v1.26.6+k3s1" => Ok(KubernetesVersion::V1_26 {
                prefix: Some(Arc::from("v")),
                patch: Some(6),
                suffix: Some(Arc::from("+k3s1")),
            }),
            "v1.27.9+k3s1" => Ok(KubernetesVersion::V1_27 {
                prefix: Some(Arc::from("v")),
                patch: Some(9),
                suffix: Some(Arc::from("+k3s1")),
            }),
            "v1.28.5+k3s1" => Ok(KubernetesVersion::V1_28 {
                prefix: Some(Arc::from("v")),
                patch: Some(5),
                suffix: Some(Arc::from("+k3s1")),
            }),
            _ => Err(()),
        }
    }
}

pub trait Kubernetes: Send + Sync {
    fn context(&self) -> &Context;
    fn kind(&self) -> Kind;
    fn id(&self) -> &str;
    fn long_id(&self) -> &Uuid;
    fn name(&self) -> &str;
    fn name_with_id(&self) -> String {
        format!("{} ({})", self.name(), self.id())
    }
    fn cluster_name(&self) -> String {
        format!("qovery-{}", self.id())
    }
    fn version(&self) -> KubernetesVersion;
    fn region(&self) -> &str;
    fn zones(&self) -> Option<Vec<&str>>;
    fn default_zone(&self) -> &str {
        match &self.zones() {
            None => "",
            Some(zones) => zones.first().unwrap_or(&""),
        }
    }
    fn cloud_provider(&self) -> &dyn CloudProvider;
    fn dns_provider(&self) -> &dyn DnsProvider;
    fn logger(&self) -> &dyn Logger;
    fn metrics_registry(&self) -> &dyn MetricsRegistry;
    fn config_file_store(&self) -> &dyn ObjectStorage;
    fn is_valid(&self) -> Result<(), Box<EngineError>>;
    fn is_network_managed_by_user(&self) -> bool;
    fn is_self_managed(&self) -> bool;
    // this method should replace kube_client
    fn q_kube_client(&self) -> Result<QubeClient, Box<EngineError>> {
        // FIXME: Create only 1 kube client per Kubernetes object instead every time this function is called
        let kubeconfig_path = self.get_kubeconfig_file_path()?;
        let kube_credentials: Vec<(String, String)> = self
            .cloud_provider()
            .credentials_environment_variables()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        QubeClient::new(
            self.get_event_details(Infrastructure(InfrastructureStep::RetrieveClusterResources)),
            kubeconfig_path.to_str().unwrap_or_default().to_string(),
            kube_credentials,
        )
    }
    // AVOID USE: to be replaced by q_kube_client
    fn kube_client(&self) -> Result<kube::Client, Box<EngineError>> {
        // FIXME: Create only 1 kube client per Kubernetes object instead every time this function is called
        let kubeconfig_path = self.get_kubeconfig_file_path()?;
        let kube_credentials: Vec<(String, String)> = self
            .cloud_provider()
            .credentials_environment_variables()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        block_on(create_kube_client(kubeconfig_path, kube_credentials.as_slice())).map_err(|err| {
            Box::new(EngineError::new_cannot_connect_to_k8s_cluster(
                self.get_event_details(Infrastructure(InfrastructureStep::RetrieveClusterResources)),
                err,
            ))
        })
    }
    fn get_kubernetes_connection(&self) -> Option<String>;
    fn create_kubeconfig_from_kubernetes_connection(&self) -> Result<(), Box<EngineError>> {
        if let Some(kubeconfig_content) = self.get_kubernetes_connection() {
            let kubeconfig_path = self.kubeconfig_local_file_path()?;
            fs::create_dir_all(
                kubeconfig_path
                    .parent()
                    .expect("Couldn't create kubeconfig folder parent path"),
            )
            .map_err(|err| {
                EngineError::new_cannot_create_file(
                    self.get_event_details(Infrastructure(InfrastructureStep::LoadConfiguration)),
                    err.into(),
                )
            })?;
            let mut file = File::create(kubeconfig_path).map_err(|err| {
                EngineError::new_cannot_create_file(
                    self.get_event_details(Infrastructure(InfrastructureStep::LoadConfiguration)),
                    err.into(),
                )
            })?;
            file.write_all(kubeconfig_content.as_bytes()).map_err(|err| {
                EngineError::new_cannot_write_file(
                    self.get_event_details(Infrastructure(InfrastructureStep::LoadConfiguration)),
                    err.into(),
                )
            })?;
        }
        Ok(())
    }
    fn cpu_architectures(&self) -> Vec<CpuArchitecture>;
    fn get_event_details(&self, stage: Stage) -> EventDetails {
        let context = self.context();
        EventDetails::new(
            Some(self.cloud_provider().kind()),
            QoveryIdentifier::new(*context.organization_long_id()),
            QoveryIdentifier::new(*context.cluster_long_id()),
            context.execution_id().to_string(),
            stage,
            Transmitter::Kubernetes(*self.long_id(), self.name().to_string()),
        )
    }

    fn get_kubeconfig_filename(&self) -> String {
        format!("{}.yaml", self.id())
    }

    fn put_kubeconfig_file_to_object_storage(&self, file_path: &Path) -> Result<(), Box<EngineError>> {
        if let Err(e) = self.config_file_store().put_object(
            self.get_bucket_name().as_str(),
            self.get_kubeconfig_filename().as_str(),
            file_path,
        ) {
            let event_details = self.get_event_details(Infrastructure(InfrastructureStep::LoadConfiguration));
            return Err(Box::new(EngineError::new_object_storage_error(event_details, e)));
        };
        Ok(())
    }

    fn delete_kubeconfig_from_object_storage(&self) -> Result<(), Box<EngineError>> {
        if let Err(e) = self
            .config_file_store()
            .delete_object(self.get_bucket_name().as_str(), self.get_kubeconfig_filename().as_str())
        {
            let event_details = self.get_event_details(Infrastructure(InfrastructureStep::LoadConfiguration));
            return Err(Box::new(EngineError::new_object_storage_error(event_details, e)));
        };
        Ok(())
    }

    fn get_bucket_name(&self) -> String {
        format!("qovery-kubeconfigs-{}", self.id())
    }

    fn kubeconfig_local_file_path(&self) -> Result<PathBuf, Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::LoadConfiguration));
        let bucket_name = self.get_bucket_name();
        let object_key = self.get_kubeconfig_filename();

        match self.get_temp_dir(event_details) {
            Ok(x) => Ok(PathBuf::from(format!("{}/{}/{}", &x, &bucket_name, &object_key))),
            Err(e) => Err(e),
        }
    }

    fn get_kubeconfig_file(&self) -> Result<PathBuf, Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::LoadConfiguration));
        let object_key = self.get_kubeconfig_filename();
        let bucket_name = self.get_bucket_name();
        let stage = Infrastructure(InfrastructureStep::RetrieveClusterConfig);

        // check if kubernetes_connection exists, meaning the local kubeconfig exists
        if self.get_kubernetes_connection().is_some() {
            return self.kubeconfig_local_file_path();
        }

        // else check if the file is present
        let local_kubeconfig = match self.kubeconfig_local_file_path() {
            Ok(kubeconfig_local_file_path) => {
                if Path::new(&kubeconfig_local_file_path).exists() {
                    match File::open(&kubeconfig_local_file_path) {
                        Ok(_) => Some(kubeconfig_local_file_path),
                        Err(err) => {
                            self.logger().log(EngineEvent::Debug(
                                self.get_event_details(stage.clone()),
                                EventMessage::new(
                                    err.to_string(),
                                    Some(format!(
                                        "Error, couldn't open {} file",
                                        kubeconfig_local_file_path.to_str().unwrap_or_default()
                                    )),
                                ),
                            ));
                            None
                        }
                    }
                } else {
                    None
                }
            }
            Err(_) => None,
        };

        // otherwise, try to get it from object storage
        let (string_path, file) = match local_kubeconfig {
            Some(local_kubeconfig_generated) => {
                let kubeconfig_file =
                    File::open(&local_kubeconfig_generated).expect("couldn't read kubeconfig file, but file exists");

                (local_kubeconfig_generated, kubeconfig_file)
            }
            None => match retry::retry(Fibonacci::from_millis(5000).take(5), || {
                match self
                    .config_file_store()
                    .get_object(bucket_name.as_str(), object_key.as_str())
                {
                    Ok(bucket_object) => {
                        let file_path = match self.kubeconfig_local_file_path() {
                            Ok(p) => p,
                            Err(e) => return OperationResult::Retry(*e),
                        };

                        // Upload kubeconfig, so we can store it
                        if let Err(err) = self
                            .context()
                            .qovery_api
                            .update_cluster_credentials(String::from_utf8_lossy(&bucket_object.value).to_string())
                        {
                            error!("Cannot update cluster credentials {}", err);
                        }

                        // Save content to file
                        // create parent dir
                        let parent_dir = match file_path.parent() {
                            Some(d) => d,
                            None => {
                                return OperationResult::Err(EngineError::new_cannot_retrieve_cluster_config_file(
                                    self.get_event_details(stage.clone()),
                                    CommandError::new_from_safe_message(format!(
                                        "Cannot get parent directory for `{}`",
                                        file_path.to_str().unwrap_or_default()
                                    )),
                                ))
                            }
                        };
                        let _ = block_on(tokio::fs::create_dir_all(parent_dir));

                        // create file
                        match std::fs::OpenOptions::new()
                            .create(true)
                            .write(true)
                            .truncate(true)
                            .open(&file_path)
                            .map_err(|err| {
                                EngineError::new_cannot_retrieve_cluster_config_file(
                                    self.get_event_details(stage.clone()),
                                    err.into(),
                                )
                            }) {
                            Ok(mut f) => {
                                if let Err(e) = f.write(bucket_object.value.as_slice()).map_err(|err| {
                                    EngineError::new_cannot_retrieve_cluster_config_file(
                                        self.get_event_details(stage.clone()),
                                        err.into(),
                                    )
                                }) {
                                    return OperationResult::Retry(e);
                                }

                                OperationResult::Ok(file_path)
                            }
                            Err(e) => OperationResult::Retry(e),
                        }
                    }
                    Err(err) => {
                        let error = EngineError::new_cannot_retrieve_cluster_config_file(
                            self.get_event_details(stage.clone()),
                            err.into(),
                        );

                        OperationResult::Retry(error)
                    }
                }
            }) {
                Ok(path) => (path.clone(), File::open(path).unwrap()), // Return file as read only mode
                Err(retry::Error { error, .. }) => {
                    // If existing cluster, it should be a warning.
                    if self.context().is_first_cluster_deployment() {
                        // if cluster first deployment, this case if normal, hence we do not log anything to end user
                        return Err(Box::new(error));
                    }

                    // It's not cluster first deployment
                    // OR we don't know if it's cluster first deployment, we do log an info log to end user.
                    self.logger().log(EngineEvent::Info(
                        event_details,
                        EventMessage::new(
                            "Cannot retrieve kubeconfig from previous installation.".to_string(),
                            Some(error.to_string()),
                        ),
                    ));

                    return Err(Box::new(error));
                }
            },
        };

        let metadata = match file.metadata() {
            Ok(metadata) => metadata,
            Err(err) => {
                let error = EngineError::new_cannot_retrieve_cluster_config_file(
                    self.get_event_details(stage),
                    CommandError::new("Error getting file metadata.".to_string(), Some(err.to_string()), None),
                );
                self.logger().log(EngineEvent::Error(error.clone(), None));
                return Err(Box::new(error));
            }
        };

        // security: ensure size match with a kubeconfig file (< 16k)
        let max_size = 16 * 1024;
        if metadata.len() > max_size {
            return Err(Box::new(EngineError::new_kubeconfig_size_security_check_error(
                event_details,
                metadata.len(),
                max_size,
            )));
        };

        let mut permissions = metadata.permissions();
        permissions.set_mode(0o400);
        if let Err(err) = std::fs::set_permissions(&string_path, permissions) {
            let error = EngineError::new_cannot_retrieve_cluster_config_file(
                self.get_event_details(stage),
                CommandError::new("Error getting file permissions.".to_string(), Some(err.to_string()), None),
            );
            self.logger().log(EngineEvent::Error(error.clone(), None));
            return Err(Box::new(error));
        }

        Ok(string_path)
    }

    fn get_kubeconfig_file_path(&self) -> Result<PathBuf, Box<EngineError>> {
        self.get_kubeconfig_file()
    }

    fn delete_local_kubeconfig_terraform(&self) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::LoadConfiguration));
        let file = self.kubeconfig_local_file_path()?;

        delete_file_if_exists(&file).map_err(|e| {
            Box::new(EngineError::new_delete_local_kubeconfig_file_error(
                event_details,
                file.to_str().unwrap_or_default(),
                e,
            ))
        })
    }

    fn delete_local_kubeconfig_object_storage_folder(&self) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::LoadConfiguration));
        let file_path = match self.kubeconfig_local_file_path() {
            Ok(p) => p,
            Err(e) => return Err(e),
        };

        delete_file_if_exists(&file_path).map_err(|e| {
            Box::new(EngineError::new_delete_local_kubeconfig_file_error(
                event_details,
                file_path.to_str().unwrap_or_default(),
                e,
            ))
        })
    }

    fn resources(&self, _environment: &Environment) -> Result<Resources, Box<EngineError>> {
        let kubernetes_config_file_path = self.get_kubeconfig_file_path()?;
        let stage = Infrastructure(InfrastructureStep::RetrieveClusterResources);

        let nodes = match kubectl_exec_get_node(
            kubernetes_config_file_path,
            self.cloud_provider().credentials_environment_variables(),
        ) {
            Ok(k) => k,
            Err(err) => {
                let error = EngineError::new_cannot_get_cluster_nodes(self.get_event_details(stage), err);

                self.logger().log(EngineEvent::Error(error.clone(), None));

                return Err(Box::new(error));
            }
        };

        let mut resources = Resources {
            free_cpu: 0.0,
            max_cpu: 0.0,
            free_ram_in_mib: 0,
            max_ram_in_mib: 0,
            free_pods: 0,
            max_pods: 0,
            running_nodes: 0,
        };

        for node in nodes.items {
            resources.free_cpu += cpu_string_to_float(node.status.allocatable.cpu);
            resources.max_cpu += cpu_string_to_float(node.status.capacity.cpu);
            resources.free_ram_in_mib += any_to_mi(node.status.allocatable.memory);
            resources.max_ram_in_mib += any_to_mi(node.status.capacity.memory);
            resources.free_pods = node.status.allocatable.pods.parse::<u32>().unwrap_or(0);
            resources.max_pods = node.status.capacity.pods.parse::<u32>().unwrap_or(0);
            resources.running_nodes += 1;
        }

        Ok(resources)
    }

    fn on_create(&self) -> Result<(), Box<EngineError>>;
    fn on_create_error(&self) -> Result<(), Box<EngineError>>;

    fn upgrade(&self) -> Result<(), Box<EngineError>> {
        // since we doesn't handle upgrade for Ec2 and getting version for them make engine bug, only check upgrade for other kinds.
        if self.kind() != Kind::Ec2 {
            let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Upgrade));

            let kubeconfig = match self.get_kubeconfig_file() {
                Ok(path) => path,
                Err(e) => return Err(e),
            };

            return match is_kubernetes_upgradable(
                kubeconfig.clone(),
                self.cloud_provider().credentials_environment_variables(),
                event_details.clone(),
            ) {
                Err(e) => Err(e),
                Ok(..) => match is_kubernetes_upgrade_required(
                    kubeconfig,
                    self.version(),
                    self.cloud_provider().credentials_environment_variables(),
                    event_details,
                    self.logger(),
                ) {
                    Ok(x) => self.upgrade_with_status(x),
                    Err(e) => Err(e),
                },
            };
        };

        Ok(())
    }

    fn check_workers_on_upgrade(&self, targeted_version: String) -> Result<(), CommandError>
    where
        Self: Sized,
    {
        send_progress_on_long_task(self, Action::Create, || {
            check_workers_upgrade_status(
                self.get_kubeconfig_file_path().expect("Unable to get Kubeconfig"),
                self.cloud_provider().credentials_environment_variables(),
                targeted_version.clone(),
            )
        })
    }

    fn check_workers_on_create(&self) -> Result<(), CommandError>
    where
        Self: Sized,
    {
        let kubeconfig = match self.get_kubeconfig_file() {
            Ok(path) => path,
            Err(e) => return Err(e.underlying_error().unwrap_or_default()),
        };

        send_progress_on_long_task(self, Action::Create, || {
            check_workers_status(&kubeconfig, self.cloud_provider().credentials_environment_variables())
        })
    }

    fn check_workers_on_pause(&self) -> Result<(), CommandError>
    where
        Self: Sized,
    {
        let kubeconfig = match self.get_kubeconfig_file() {
            Ok(path) => path,
            Err(e) => return Err(e.underlying_error().unwrap_or_default()),
        };

        send_progress_on_long_task(self, Action::Create, || {
            check_workers_pause(&kubeconfig, self.cloud_provider().credentials_environment_variables())
        })
    }
    fn upgrade_with_status(&self, kubernetes_upgrade_status: KubernetesUpgradeStatus) -> Result<(), Box<EngineError>>;
    fn on_upgrade(&self) -> Result<(), Box<EngineError>>;
    fn on_upgrade_error(&self) -> Result<(), Box<EngineError>>;
    fn on_pause(&self) -> Result<(), Box<EngineError>>;
    fn on_pause_error(&self) -> Result<(), Box<EngineError>>;
    fn on_delete(&self) -> Result<(), Box<EngineError>>;
    fn on_delete_error(&self) -> Result<(), Box<EngineError>>;
    fn get_temp_dir(&self, event_details: EventDetails) -> Result<String, Box<EngineError>> {
        workspace_directory(
            self.context().workspace_root_dir(),
            self.context().execution_id(),
            format!("bootstrap/{}", self.id()),
        )
        .map_err(|err| {
            Box::new(EngineError::new_cannot_get_workspace_directory(
                event_details,
                CommandError::new("Error creating workspace directory.".to_string(), Some(err.to_string()), None),
            ))
        })
    }

    fn delete_crashlooping_pods(
        &self,
        namespace: Option<&str>,
        selector: Option<&str>,
        restarted_min_count: Option<usize>,
        envs: Vec<(&str, &str)>,
        stage: Stage,
    ) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(stage);

        match self.get_kubeconfig_file() {
            Err(e) => return Err(e),
            Ok(config_path) => match kubectl_get_crash_looping_pods(
                &config_path,
                namespace,
                selector,
                restarted_min_count,
                envs.clone(),
            ) {
                Ok(pods) => {
                    for pod in pods {
                        if let Err(e) = kubectl_exec_delete_pod(
                            &config_path,
                            pod.metadata.namespace.as_str(),
                            pod.metadata.name.as_str(),
                            envs.clone(),
                        ) {
                            return Err(Box::new(EngineError::new_k8s_cannot_delete_pod(
                                event_details,
                                pod.metadata.name.to_string(),
                                e,
                            )));
                        }
                    }
                }
                Err(e) => {
                    return Err(Box::new(EngineError::new_k8s_cannot_get_crash_looping_pods(event_details, e)));
                }
            },
        };

        Ok(())
    }

    fn delete_completed_jobs(&self, envs: Vec<(&str, &str)>, stage: Stage) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(stage);

        match self.get_kubeconfig_file() {
            Err(e) => return Err(e),
            Ok(config_path) => {
                if let Err(e) = kubectl_delete_completed_jobs(config_path, envs) {
                    return Err(Box::new(EngineError::new_k8s_cannot_delete_completed_jobs(event_details, e)));
                };
            }
        }

        Ok(())
    }

    fn update_vault_config(
        &self,
        event_details: EventDetails,
        qovery_terraform_config_file: String,
        cluster_secrets: ClusterSecrets,
        kubeconfig_file_path: Option<&Path>,
    ) -> Result<(), Box<EngineError>>;

    fn advanced_settings(&self) -> &ClusterAdvancedSettings;

    fn customer_helm_charts_override(&self) -> Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>>;
}

pub trait KubernetesNode {
    fn instance_type(&self) -> &str;
    fn as_any(&self) -> &dyn Any;
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, EnumIter)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Kind {
    Eks,
    Ec2,
    ScwKapsule,
    Gke,
    EksSelfManaged,
    GkeSelfManaged,
    ScwSelfManaged,
}

impl Kind {
    pub fn get_cloud_provider_kind(&self) -> CloudProviderKind {
        match self {
            Kind::Eks | Kind::EksSelfManaged | Kind::Ec2 => CloudProviderKind::Aws,
            Kind::ScwKapsule | Kind::ScwSelfManaged => CloudProviderKind::Scw,
            Kind::Gke | Kind::GkeSelfManaged => CloudProviderKind::Gcp,
        }
    }
}

impl Display for Kind {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Kind::Eks => "EKS",
            Kind::Ec2 => "K3S",
            Kind::ScwKapsule => "ScwKapsule",
            Kind::Gke => "GKE",
            Kind::EksSelfManaged => "EKS Self Managed",
            Kind::GkeSelfManaged => "GKE Self Managed",
            Kind::ScwSelfManaged => "Scw Self Managed",
        })
    }
}

#[derive(Debug)]
pub struct Resources {
    pub free_cpu: f32,
    pub max_cpu: f32,
    pub free_ram_in_mib: u32,
    pub max_ram_in_mib: u32,
    pub free_pods: u32,
    pub max_pods: u32,
    pub running_nodes: u32,
}

pub fn event_details(
    cloud_provider: &dyn CloudProvider,
    kubernetes_id: Uuid,
    kubernetes_name: String,
    context: &Context,
) -> EventDetails {
    EventDetails::new(
        Some(cloud_provider.kind()),
        QoveryIdentifier::new(*context.organization_long_id()),
        QoveryIdentifier::new(*context.cluster_long_id()),
        context.execution_id().to_string(),
        Infrastructure(InfrastructureStep::LoadConfiguration),
        Transmitter::Kubernetes(kubernetes_id, kubernetes_name),
    )
}

pub fn uninstall_cert_manager<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
    event_details: EventDetails,
    logger: &dyn Logger,
) -> Result<(), Box<EngineError>>
where
    P: AsRef<Path>,
{
    // https://cert-manager.io/docs/installation/uninstall/kubernetes/

    let cert_manager_objects = vec![
        "Issuers",
        "ClusterIssuers",
        "Certificates",
        "CertificateRequests",
        "Orders",
        "Challenges",
    ];

    for object in cert_manager_objects {
        // check resource exist first
        if let Err(e) = kubectl_exec_count_all_objects(&kubernetes_config, object, envs.clone()) {
            logger.log(EngineEvent::Warning(
                event_details.clone(),
                EventMessage::new(
                    format!("Encountering issues while trying to get objects kind {object}",),
                    Some(e.message(ErrorMessageVerbosity::FullDetails)),
                ),
            ));
            continue;
        }

        // delete if resource exists
        match retry::retry(
            Fibonacci::from_millis(5000).take(3),
            || match kubectl_delete_objects_in_all_namespaces(&kubernetes_config, object, envs.clone()) {
                Ok(_) => OperationResult::Ok(()),
                Err(e) => {
                    logger.log(EngineEvent::Warning(
                        event_details.clone(),
                        EventMessage::new(format!("Failed to delete all {object} objects, retrying...",), None),
                    ));
                    OperationResult::Retry(e)
                }
            },
        ) {
            Ok(_) => {}
            Err(retry::Error { error, .. }) => {
                let engine_error = EngineError::new_cannot_uninstall_helm_chart(
                    event_details.clone(),
                    "Cert-Manager".to_string(),
                    object.to_string(),
                    error,
                );

                logger.log(EngineEvent::Warning(
                    event_details.clone(),
                    EventMessage::new_from_engine_error(engine_error),
                ));
            }
        }
    }

    // delete qovery apiservice deployed by Qvery webhook to avoid namespace in infinite Terminating state
    let _ = kubectl_delete_apiservice(kubernetes_config, "release=qovery-cert-manager-webhook", envs);

    Ok(())
}

impl NodeGroupsWithDesiredState {
    pub fn new_from_node_groups(
        nodegroup: &NodeGroups,
        desired_nodes: i32,
        enable_desired_nodes: bool,
    ) -> NodeGroupsWithDesiredState {
        NodeGroupsWithDesiredState {
            name: nodegroup.name.clone(),
            id: nodegroup.id.clone(),
            min_nodes: nodegroup.min_nodes,
            max_nodes: nodegroup.max_nodes,
            desired_size: desired_nodes,
            enable_desired_size: enable_desired_nodes,
            instance_type: nodegroup.instance_type.clone(),
            disk_size_in_gib: nodegroup.disk_size_in_gib,
            instance_architecture: nodegroup.instance_architecture,
        }
    }
}

pub fn is_kubernetes_upgrade_required<P>(
    kubernetes_config: P,
    requested_version: KubernetesVersion,
    envs: Vec<(&str, &str)>,
    event_details: EventDetails,
    logger: &dyn Logger,
) -> Result<KubernetesUpgradeStatus, Box<EngineError>>
where
    P: AsRef<Path>,
{
    // check master versions
    let version_result = retry::retry(Fixed::from_millis(5 * 1000).take(5), || {
        let v = match kubectl_exec_version(&kubernetes_config, envs.clone()) {
            Ok(v) => v,
            Err(e) => {
                return OperationResult::Retry(EngineError::new_cannot_execute_k8s_exec_version(
                    event_details.clone(),
                    e,
                ))
            }
        };
        let raw_version = format!("{}.{}", v.server_version.major, v.server_version.minor);
        match VersionsNumber::from_str(raw_version.as_str()) {
            Ok(vn) => OperationResult::Ok(vn),
            Err(_) => OperationResult::Err(EngineError::new_cannot_determine_k8s_master_version(
                event_details.clone(),
                raw_version.to_string(),
            )),
        }
    });

    let masters_version = match version_result {
        Ok(v) => v,
        Err(retry::Error { error, .. }) => return Err(Box::new(error)),
    };

    // check workers versions
    let mut workers_version: Vec<VersionsNumber> = vec![];
    let nodes = match kubectl_exec_get_node(kubernetes_config, envs) {
        Ok(n) => n,
        Err(e) => return Err(Box::new(EngineError::new_cannot_get_cluster_nodes(event_details, e))),
    };

    for node in nodes.items {
        // check kubelet version
        match VersionsNumber::from_str(node.status.node_info.kubelet_version.as_str()) {
            Ok(vn) => workers_version.push(vn),
            Err(_) => {
                return Err(Box::new(EngineError::new_cannot_determine_k8s_kubelet_worker_version(
                    event_details,
                    node.status.node_info.kubelet_version.to_string(),
                )))
            }
        }

        // check kube-proxy version
        match VersionsNumber::from_str(node.status.node_info.kube_proxy_version.as_str()) {
            Ok(vn) => workers_version.push(vn),
            Err(_) => {
                return Err(Box::new(EngineError::new_cannot_determine_k8s_kube_proxy_version(
                    event_details,
                    node.status.node_info.kube_proxy_version.to_string(),
                )))
            }
        }
    }

    check_kubernetes_upgrade_status(requested_version, masters_version, workers_version, event_details, logger)
}

pub fn is_kubernetes_upgradable<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
    event_details: EventDetails,
) -> Result<(), Box<EngineError>>
where
    P: AsRef<Path>,
{
    match kubernetes_get_all_pdbs(kubernetes_config, envs, None) {
        Ok(pdbs) => match pdbs.items.is_some() {
            false => Ok(()),
            true => {
                for pdb in pdbs.items.unwrap() {
                    if pdb.status.current_healthy < pdb.status.desired_healthy {
                        return Err(Box::new(EngineError::new_k8s_pod_disruption_budget_invalid_state(
                            event_details,
                            pdb.metadata.name,
                        )));
                    }
                }
                Ok(())
            }
        },
        Err(err) => Err(Box::new(EngineError::new_k8s_cannot_retrieve_pods_disruption_budget(
            event_details,
            err,
        ))),
    }
}

pub fn check_workers_upgrade_status<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
    target_version: String,
) -> Result<(), CommandError>
where
    P: AsRef<Path>,
{
    let result = retry::retry(Fixed::from_millis(10000).take(360), || {
        match kubectl_exec_get_node(kubernetes_config.as_ref(), envs.clone()) {
            Err(e) => OperationResult::Retry(e),
            Ok(nodes) => {
                for node in nodes.items.iter() {
                    if !node.status.node_info.kubelet_version.contains(&target_version[..4]) {
                        return OperationResult::Retry(CommandError::new_from_safe_message(
                            "There are still not upgraded nodes.".to_string(),
                        ));
                    }
                }
                OperationResult::Ok(())
            }
        }
    });

    return match result {
        Ok(_) => match check_workers_status(kubernetes_config.as_ref(), envs.clone()) {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        },
        Err(retry::Error { error, .. }) => Err(error),
    };
}

pub fn check_workers_status<P>(kubernetes_config: P, envs: Vec<(&str, &str)>) -> Result<(), CommandError>
where
    P: AsRef<Path>,
{
    let result = retry::retry(Fixed::from_millis(10000).take(60), || {
        match kubectl_exec_get_node(kubernetes_config.as_ref(), envs.clone()) {
            Err(e) => OperationResult::Retry(e),
            Ok(nodes) => {
                let mut conditions: Vec<KubernetesNodeCondition> = Vec::new();
                for node in nodes.items.into_iter() {
                    conditions.extend(node.status.conditions.into_iter());
                }

                for condition in conditions.iter() {
                    if condition.condition_type == "Ready" && condition.status != "True" {
                        return OperationResult::Retry(CommandError::new_from_safe_message(
                            "There are still not ready worker nodes.".to_string(),
                        ));
                    }
                }
                OperationResult::Ok(())
            }
        }
    });

    match result {
        Ok(_) => Ok(()),
        Err(retry::Error { error, .. }) => Err(error),
    }
}

pub fn check_workers_pause<P>(kubernetes_config: P, envs: Vec<(&str, &str)>) -> Result<(), CommandError>
where
    P: AsRef<Path>,
{
    let result = retry::retry(Fixed::from_millis(10000).take(60), || {
        match kubectl_exec_get_node(kubernetes_config.as_ref(), envs.clone()) {
            //TODO: handle error properly
            Err(_) => OperationResult::Ok(()),
            Ok(nodes) => {
                if !nodes.items.is_empty() {
                    return OperationResult::Retry(CommandError::new_from_safe_message(
                        "There are still not paused worker nodes.".to_string(),
                    ));
                }

                OperationResult::Ok(())
            }
        }
    });

    match result {
        Ok(_) => Ok(()),
        Err(retry::Error { error, .. }) => Err(error),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum KubernetesNodesType {
    Masters,
    Workers,
}

#[derive(Debug)]
pub struct KubernetesUpgradeStatus {
    pub required_upgrade_on: Option<KubernetesNodesType>,
    pub requested_version: VersionsNumber,
    pub deployed_masters_version: VersionsNumber,
    pub deployed_workers_version: VersionsNumber,
    pub older_masters_version_detected: bool,
    pub older_workers_version_detected: bool,
}

/// Check if Kubernetes cluster elements are requiring an upgrade
///
/// It will gives useful info:
/// * versions of masters
/// * versions of workers (the oldest)
/// * which type of nodes should be upgraded in priority
/// * is the requested version is older than the current deployed
///
fn check_kubernetes_upgrade_status(
    requested_version: KubernetesVersion,
    deployed_masters_version: VersionsNumber,
    deployed_workers_version: Vec<VersionsNumber>,
    event_details: EventDetails,
    logger: &dyn Logger,
) -> Result<KubernetesUpgradeStatus, Box<EngineError>> {
    let mut total_workers = 0;
    let mut non_up_to_date_workers = 0;
    let mut required_upgrade_on = None;
    let mut older_masters_version_detected = false;
    let mut older_workers_version_detected = false;

    let wished_version: VersionsNumber = requested_version.into();

    // check master versions
    match compare_kubernetes_cluster_versions_for_upgrade(&deployed_masters_version, &wished_version) {
        Ok(x) => {
            if let Some(msg) = x.message {
                logger.log(EngineEvent::Info(event_details.clone(), EventMessage::new_from_safe(msg)));
            };
            if x.older_version_detected {
                older_masters_version_detected = x.older_version_detected;
            }
            if x.upgraded_required {
                required_upgrade_on = Some(KubernetesNodesType::Masters);
            }
        }
        Err(e) => {
            return Err(Box::new(
                EngineError::new_k8s_version_upgrade_deployed_vs_requested_versions_inconsistency(
                    event_details,
                    deployed_masters_version,
                    wished_version,
                    e,
                ),
            ))
        }
    };

    // check workers versions
    if deployed_workers_version.is_empty() {
        logger.log(EngineEvent::Info(
            event_details,
            EventMessage::new_from_safe(
                "No worker nodes found, can't check if upgrade is required for workers".to_string(),
            ),
        ));

        return Ok(KubernetesUpgradeStatus {
            required_upgrade_on,
            requested_version: wished_version,
            deployed_masters_version: deployed_masters_version.clone(),
            deployed_workers_version: deployed_masters_version,
            older_masters_version_detected,
            older_workers_version_detected,
        });
    }

    let mut workers_oldest_version = deployed_workers_version[0].clone();

    for node in deployed_workers_version {
        total_workers += 1;
        match compare_kubernetes_cluster_versions_for_upgrade(&node, &wished_version) {
            Ok(x) => {
                if x.older_version_detected {
                    older_workers_version_detected = x.older_version_detected;
                    workers_oldest_version = node.clone();
                };
                if x.upgraded_required {
                    workers_oldest_version = node;
                    match required_upgrade_on {
                        Some(KubernetesNodesType::Masters) => {}
                        _ => required_upgrade_on = Some(KubernetesNodesType::Workers),
                    };
                };
                non_up_to_date_workers += 1;
            }
            Err(e) => {
                return Err(Box::new(
                    EngineError::new_k8s_version_upgrade_deployed_vs_requested_versions_inconsistency(
                        event_details,
                        node,
                        wished_version,
                        e,
                    ),
                ))
            }
        }
    }

    logger.log(EngineEvent::Info(
        event_details,
        EventMessage::new_from_safe(match &required_upgrade_on {
            None => "All workers are up to date, no upgrade required".to_string(),
            Some(node_type) => match node_type {
                KubernetesNodesType::Masters => "Kubernetes master upgrade required".to_string(),
                KubernetesNodesType::Workers => format!(
                    "Kubernetes workers upgrade required, need to update {non_up_to_date_workers}/{total_workers} nodes"
                ),
            },
        }),
    ));

    Ok(KubernetesUpgradeStatus {
        required_upgrade_on,
        requested_version: wished_version,
        deployed_masters_version,
        deployed_workers_version: workers_oldest_version,
        older_masters_version_detected,
        older_workers_version_detected,
    })
}

pub struct CompareKubernetesStatus {
    pub upgraded_required: bool,
    pub older_version_detected: bool,
    pub message: Option<String>,
}

pub fn compare_kubernetes_cluster_versions_for_upgrade(
    deployed_version: &VersionsNumber,
    wished_version: &VersionsNumber,
) -> Result<CompareKubernetesStatus, CommandError> {
    let mut messages: Vec<&str> = Vec::new();
    let mut upgrade_required = CompareKubernetesStatus {
        upgraded_required: false,
        older_version_detected: false,
        message: None,
    };

    let deployed_minor_version = match &deployed_version.minor {
        Some(v) => v,
        None => {
            return Err(CommandError::new_from_safe_message(
                "deployed kubernetes minor version was missing and is missing".to_string(),
            ))
        }
    };

    let wished_minor_version = match &wished_version.minor {
        Some(v) => v,
        None => {
            return Err(CommandError::new_from_safe_message(
                "wished kubernetes minor version was expected and is missing".to_string(),
            ))
        }
    };

    if wished_version.major > deployed_version.major {
        upgrade_required.upgraded_required = true;
        messages.push("Kubernetes major version change detected");
    }

    if wished_version.major < deployed_version.major {
        upgrade_required.upgraded_required = false;
        upgrade_required.older_version_detected = true;
        messages.push("Older Kubernetes major version detected");
    }

    if wished_minor_version > deployed_minor_version {
        upgrade_required.upgraded_required = true;
        messages.push("Kubernetes minor version change detected");
    }

    if wished_minor_version < deployed_minor_version {
        upgrade_required.upgraded_required = false;
        upgrade_required.older_version_detected = true;
        messages.push("Older Kubernetes minor version detected");
    }

    let mut final_message = "Kubernetes cluster upgrade is not required".to_string();
    if upgrade_required.upgraded_required {
        let old = format!("{}.{}", deployed_version.major, deployed_minor_version);
        let new = format!("{}.{}", wished_version.major, wished_minor_version);
        final_message = format!("Kubernetes cluster upgrade is required {old} -> {new} !!!");
    }
    messages.push(final_message.as_str());
    upgrade_required.message = Some(messages.join(". "));

    Ok(upgrade_required)
}

pub trait InstanceType {
    fn to_cloud_provider_format(&self) -> String;
    fn is_instance_allowed(&self) -> bool;
    fn is_arm_instance(&self) -> bool;
    fn is_instance_cluster_allowed(&self) -> bool;
}

impl NodeGroups {
    pub fn new(
        group_name: String,
        min_nodes: i32,
        max_nodes: i32,
        instance_type: String,
        disk_size_in_gib: i32,
        instance_architecture: CpuArchitecture,
    ) -> Result<Self, CommandError> {
        if min_nodes > max_nodes {
            return Err(CommandError::new_from_safe_message(format!(
                "The number of minimum nodes ({}) for group name {} is higher than maximum nodes ({})",
                &group_name, &min_nodes, &max_nodes
            )));
        }

        Ok(NodeGroups {
            name: group_name,
            id: None,
            min_nodes,
            max_nodes,
            instance_type,
            disk_size_in_gib,
            desired_nodes: None,
            instance_architecture,
        })
    }

    pub fn to_ec2_instance(&self) -> InstanceEc2 {
        InstanceEc2 {
            instance_type: self.instance_type.clone(),
            disk_size_in_gib: self.disk_size_in_gib,
            instance_architecture: self.instance_architecture,
        }
    }

    pub fn set_desired_nodes(&mut self, desired_nodes: i32) {
        // desired nodes can't be lower than min nodes
        if desired_nodes < self.min_nodes {
            self.desired_nodes = Some(self.min_nodes)
            // desired nodes can't be higher than max nodes
        } else if desired_nodes > self.max_nodes {
            self.desired_nodes = Some(self.max_nodes)
        } else {
            self.desired_nodes = Some(desired_nodes)
        }
    }
}

impl InstanceEc2 {
    pub fn new(instance_type: String, disk_size_in_gib: i32, instance_architecture: CpuArchitecture) -> InstanceEc2 {
        InstanceEc2 {
            instance_type,
            disk_size_in_gib,
            instance_architecture,
        }
    }
}

/// TODO(benjaminch): to be refactored with similar function in services.rs
/// This function call (start|pause|delete)_in_progress function every 10 seconds when a
/// long blocking task is running.
pub fn send_progress_on_long_task<K, R, F>(kubernetes: &K, action: Action, long_task: F) -> R
where
    K: Kubernetes,
    F: Fn() -> R,
{
    let waiting_message = match action {
        Action::Create => Some(format!(
            "Infrastructure '{}' deployment is in progress...",
            kubernetes.name_with_id()
        )),
        Action::Pause => Some(format!(
            "Infrastructure '{}' pause is in progress...",
            kubernetes.name_with_id()
        )),
        Action::Delete => Some(format!(
            "Infrastructure '{}' deletion is in progress...",
            kubernetes.name_with_id()
        )),
        Action::Restart => None,
    };

    send_progress_on_long_task_with_message(kubernetes, waiting_message, action, long_task)
}

/// TODO(benjaminch): to be refactored with similar function in services.rs
/// This function call (start|pause|delete)_in_progress function every 10 seconds when a
/// long blocking task is running.
pub fn send_progress_on_long_task_with_message<K, R, F>(
    kubernetes: &K,
    waiting_message: Option<String>,
    action: Action,
    long_task: F,
) -> R
where
    K: Kubernetes,
    F: Fn() -> R,
{
    let logger = kubernetes.logger().clone_dyn();
    let event_details = kubernetes.get_event_details(Infrastructure(InfrastructureStep::Create));

    let (tx, rx) = mpsc::channel();
    let span = Span::current();

    // monitor thread to notify user while the blocking task is executed
    let handle = thread::Builder::new()
        .name("infra-task-monitor".to_string())
        .spawn(move || {
            // stop the thread when the blocking task is done
            let _span = span.enter();
            let waiting_message = waiting_message.unwrap_or_else(|| "no message ...".to_string());

            loop {
                // do notify users here
                let event_details = Clone::clone(&event_details);
                let event_message = EventMessage::new_from_safe(waiting_message.to_string());

                match action {
                    Action::Create => {
                        logger.log(EngineEvent::Info(
                            EventDetails::clone_changing_stage(
                                event_details,
                                Infrastructure(InfrastructureStep::Create),
                            ),
                            event_message,
                        ));
                    }
                    Action::Pause => {
                        logger.log(EngineEvent::Info(
                            EventDetails::clone_changing_stage(
                                event_details,
                                Infrastructure(InfrastructureStep::Pause),
                            ),
                            event_message,
                        ));
                    }
                    Action::Delete => {
                        logger.log(EngineEvent::Info(
                            EventDetails::clone_changing_stage(
                                event_details,
                                Infrastructure(InfrastructureStep::Delete),
                            ),
                            event_message,
                        ));
                    }
                    Action::Restart => {
                        // restart is not implemented yet
                    }
                };

                thread::sleep(Duration::from_secs(30));

                // watch for thread termination
                match rx.try_recv() {
                    Ok(_) | Err(TryRecvError::Disconnected) => break,
                    Err(TryRecvError::Empty) => {}
                }
            }
        });

    let blocking_task_result = long_task();
    let _ = tx.send(());
    let _ = handle.map(|it| it.join());

    blocking_task_result
}

pub fn validate_k8s_required_cpu_and_burstable(
    total_cpu: String,
    cpu_burst: String,
) -> Result<CpuLimits, CommandError> {
    let total_cpu_float = convert_k8s_cpu_value_to_f32(total_cpu.clone())?;
    let cpu_burst_float = convert_k8s_cpu_value_to_f32(cpu_burst.clone())?;
    let mut set_cpu_burst = cpu_burst;

    if cpu_burst_float < total_cpu_float {
        set_cpu_burst = total_cpu.clone();
    }

    Ok(CpuLimits {
        cpu_limit: set_cpu_burst,
        cpu_request: total_cpu,
    })
}

/// TODO(benjaminch): deprecate this function and use plain KubernetesCpuRessourceUnit
pub fn convert_k8s_cpu_value_to_f32(value: String) -> Result<f32, CommandError> {
    if value.ends_with('m') {
        let mut value_number_string = value;
        value_number_string.pop();
        return match value_number_string.parse::<f32>() {
            Ok(n) => {
                Ok(n * 0.001) // return in milli cpu the value
            }
            Err(e) => Err(CommandError::new(
                format!("Error while trying to parse `{}` to float 32.", value_number_string.as_str()),
                Some(e.to_string()),
                None,
            )),
        };
    }

    match value.parse::<f32>() {
        Ok(n) => Ok(n),
        Err(e) => Err(CommandError::new(
            format!("Error while trying to parse `{}` to float 32.", value.as_str()),
            Some(e.to_string()),
            None,
        )),
    }
}

pub async fn kube_does_secret_exists(kube: &kube::Client, name: &str, namespace: &str) -> Result<bool, Error> {
    let item: Api<Secret> = Api::namespaced(kube.clone(), namespace);
    match item.get(name).await {
        Ok(_) => Ok(true),
        Err(e) => match e {
            Error::Api(api_err) if api_err.code == 404 => Ok(false),
            _ => Err(e),
        },
    }
}

pub async fn kube_list_services(
    kube: &kube::Client,
    namespace_name: Option<&str>,
    labels_selector: Option<&str>,
) -> Result<ObjectList<Service>, CommandError> {
    let client: Api<Service> = match namespace_name {
        Some(namespace_name) => Api::namespaced(kube.clone(), namespace_name),
        None => Api::all(kube.clone()),
    };

    let params = match labels_selector {
        Some(x) => ListParams::default().labels(x),
        None => ListParams::default(),
    };

    match client.list(&params).await {
        Ok(x) => Ok(x),
        Err(e) => Err(CommandError::new(
            "Error while trying to get kubernetes services".to_string(),
            Some(e.to_string()),
            None,
        )),
    }
}

pub fn filter_svc_loadbalancers(load_balancers: ObjectList<Service>) -> Vec<Service> {
    let mut filtered_load_balancers = Vec::new();

    for service in load_balancers.into_iter() {
        let spec = match &service.spec {
            Some(x) => x,
            None => continue,
        };

        match &spec.type_ {
            Some(x) if x == "LoadBalancer" => filtered_load_balancers.push(service),
            _ => continue,
        };
    }

    filtered_load_balancers
}

pub async fn kube_create_namespace_if_not_exists(
    kube: &kube::Client,
    namespace_name: &str,
    labels: BTreeMap<String, String>,
) -> Result<(), Error> {
    let ns_api = Api::all(kube.clone());
    let namespace = Namespace {
        metadata: ObjectMeta {
            name: Some(namespace_name.to_string()),
            labels: Some(labels.clone()),
            ..Default::default()
        },
        spec: None,
        status: None,
    };

    // create namespace
    if let Err(e) = ns_api.create(&PostParams::default(), &namespace).await {
        match e {
            // namespace already exists
            Error::Api(api_err) if api_err.code == 409 => {}
            _ => return Err(e),
        }
    };

    // We patch the labels to make sure they are up to date
    let patch_labels = json!({
        "metadata": {
            "labels": labels
        }
    });
    ns_api
        .patch(namespace_name, &PatchParams::default(), &Patch::Merge(patch_labels))
        .await?;

    Ok(())
}

pub async fn kube_copy_secret_to_another_namespace(
    kube: &kube::Client,
    name: &str,
    namespace_src: &str,
    namespace_dest: &str,
) -> Result<(), Error> {
    let post_param = PostParams::default();

    let secret_src: Api<Secret> = Api::namespaced(kube.clone(), namespace_src);
    let mut secret_content = secret_src.get(name).await?;
    secret_content.metadata.namespace = Some(namespace_dest.to_string());
    secret_content.metadata.resource_version = None;
    secret_content.metadata.uid = None;
    secret_content.metadata.creation_timestamp = None;

    let secret_dest: Api<Secret> = Api::namespaced(kube.clone(), namespace_dest);
    match secret_dest.create(&post_param, &secret_content).await {
        Ok(_) => Ok(()),
        Err(kube_err) => match kube_err {
            Error::Api(e) if e.code == 409 => Ok(()),
            _ => Err(kube_err),
        },
    }
}

#[cfg(test)]
mod tests {
    use k8s_openapi::api::core::v1::{Service, ServiceSpec};
    use kube::core::{ListMeta, ObjectList, ObjectMeta};
    use std::collections::BTreeMap;

    use crate::cloud_provider::kubernetes;
    use crate::cloud_provider::kubernetes::{
        check_kubernetes_upgrade_status, compare_kubernetes_cluster_versions_for_upgrade, convert_k8s_cpu_value_to_f32,
        filter_svc_loadbalancers, validate_k8s_required_cpu_and_burstable, KubernetesNodesType,
    };
    use crate::cloud_provider::kubernetes::{
        kube_copy_secret_to_another_namespace, kube_create_namespace_if_not_exists, kube_does_secret_exists,
        kube_list_services, KubernetesVersion as K8sVersion,
    };
    use crate::cloud_provider::models::CpuLimits;
    use crate::cmd::structs::{KubernetesList, KubernetesNode, KubernetesVersion};
    use crate::events::{EventDetails, InfrastructureStep, Stage, Transmitter};
    use crate::io_models::QoveryIdentifier;
    use crate::logger::StdIoLogger;
    use crate::models::types::VersionsNumber;
    use crate::runtime::block_on;
    use crate::utilities::create_kube_client;
    use std::env;
    use std::str::FromStr;
    use std::sync::Arc;
    use strum::IntoEnumIterator;
    use uuid::Uuid;

    pub fn kubeconfig_path() -> String {
        env::var("HOME").unwrap() + "/.kube/config"
    }

    pub fn get_svc_template() -> ObjectList<Service> {
        ObjectList {
            metadata: ListMeta { ..Default::default() },
            items: vec![
                Service {
                    metadata: ObjectMeta {
                        name: Some("loadbalancer".to_string()),
                        namespace: Some("ns0".to_string()),
                        ..Default::default()
                    },
                    spec: Some(ServiceSpec {
                        type_: Some("LoadBalancer".to_string()),
                        ..Default::default()
                    }),
                    status: None,
                },
                Service {
                    metadata: ObjectMeta {
                        name: Some("clusterip".to_string()),
                        namespace: Some("ns1".to_string()),
                        ..Default::default()
                    },
                    spec: Some(ServiceSpec {
                        type_: Some("ClusterIp".to_string()),
                        ..Default::default()
                    }),
                    status: None,
                },
            ],
        }
    }

    #[test]
    #[cfg(feature = "test-local-kube")]
    pub fn k8s_get_services() {
        let kube_client = block_on(create_kube_client(kubeconfig_path(), &[])).unwrap();
        let svcs = block_on(kube_list_services(&kube_client, None, None));
        assert!(svcs.is_ok());
        assert!(!svcs.unwrap().items.is_empty());
    }

    #[test]
    pub fn k8s_get_filter_aws_loadbalancers() {
        let svcs = get_svc_template();
        let filtered_lbs = filter_svc_loadbalancers(svcs);
        assert_eq!(filtered_lbs.len(), 1);
        assert_eq!(filtered_lbs[0].clone().metadata.name.unwrap(), "loadbalancer");
        assert_eq!(filtered_lbs[0].clone().metadata.namespace.unwrap(), "ns0");
    }

    #[test]
    #[cfg(feature = "test-local-kube")]
    pub fn k8s_create_namespace() {
        let kube_client = block_on(create_kube_client(kubeconfig_path(), &[])).unwrap();
        assert!(block_on(kube_create_namespace_if_not_exists(
            &kube_client,
            "qovery-test-ns",
            BTreeMap::from([("qovery.io/namespace-type".to_string(), "development".to_string())])
        ))
        .is_ok());
    }

    #[test]
    #[cfg(feature = "test-local-kube")]
    pub fn k8s_does_secret_exists_test() {
        let kube_client = block_on(create_kube_client(kubeconfig_path(), &[])).unwrap();
        let res = block_on(kube_does_secret_exists(&kube_client, "k3s-serving", "kube-system")).unwrap();
        assert!(res);
    }

    #[test]
    #[cfg(feature = "test-local-kube")]
    pub fn k8s_copy_secret_test() {
        let kube_client = block_on(create_kube_client(kubeconfig_path(), &[])).unwrap();
        block_on(kube_copy_secret_to_another_namespace(
            &kube_client,
            "k3s-serving",
            "kube-system",
            "default",
        ))
        .unwrap();
    }

    #[test]
    pub fn check_kubernetes_upgrade_method() {
        let version_1_25: VersionsNumber = K8sVersion::V1_25 {
            prefix: None,
            patch: None,
            suffix: None,
        }
        .into();
        let version_1_26: VersionsNumber = K8sVersion::V1_26 {
            prefix: None,
            patch: None,
            suffix: None,
        }
        .into();
        let event_details = EventDetails::new(
            None,
            QoveryIdentifier::new_random(),
            QoveryIdentifier::new_random(),
            Uuid::new_v4().to_string(),
            Stage::Infrastructure(InfrastructureStep::Upgrade),
            Transmitter::Kubernetes(Uuid::new_v4(), "test".to_string()),
        );
        let logger = StdIoLogger::new();

        // test full cluster upgrade (masters + workers)
        let result = check_kubernetes_upgrade_status(
            K8sVersion::V1_26 {
                prefix: None,
                patch: None,
                suffix: None,
            },
            version_1_25.clone(),
            vec![version_1_25.clone()],
            event_details.clone(),
            &logger,
        )
        .unwrap();
        assert_eq!(result.required_upgrade_on.unwrap(), KubernetesNodesType::Masters); // master should be performed first
        assert_eq!(result.deployed_masters_version, version_1_25);
        assert_eq!(result.deployed_workers_version, version_1_25);
        assert!(!result.older_masters_version_detected);
        assert!(!result.older_workers_version_detected);
        let result = check_kubernetes_upgrade_status(
            K8sVersion::V1_26 {
                prefix: None,
                patch: None,
                suffix: None,
            },
            version_1_26.clone(),
            vec![version_1_25.clone()],
            event_details.clone(),
            &logger,
        )
        .unwrap();
        assert_eq!(result.required_upgrade_on.unwrap(), KubernetesNodesType::Workers); // then workers
        assert_eq!(result.deployed_masters_version, version_1_26);
        assert_eq!(result.deployed_workers_version, version_1_25);
        assert!(!result.older_masters_version_detected);
        assert!(!result.older_workers_version_detected);

        // everything is up to date, no upgrade required
        let result = check_kubernetes_upgrade_status(
            K8sVersion::V1_26 {
                prefix: None,
                patch: None,
                suffix: None,
            },
            version_1_26.clone(),
            vec![version_1_26.clone()],
            event_details.clone(),
            &logger,
        )
        .unwrap();
        assert!(result.required_upgrade_on.is_none());
        assert!(!result.older_masters_version_detected);
        assert!(!result.older_workers_version_detected);

        // downgrade should be detected
        let result = check_kubernetes_upgrade_status(
            K8sVersion::V1_25 {
                prefix: None,
                patch: None,
                suffix: None,
            },
            version_1_26.clone(),
            vec![version_1_26.clone()],
            event_details.clone(),
            &logger,
        )
        .unwrap();
        assert!(result.required_upgrade_on.is_none());
        assert!(result.older_masters_version_detected);
        assert!(result.older_workers_version_detected);

        // mixed workers version
        let result = check_kubernetes_upgrade_status(
            K8sVersion::V1_26 {
                prefix: None,
                patch: None,
                suffix: None,
            },
            version_1_26.clone(),
            vec![version_1_26.clone(), version_1_25.clone()],
            event_details,
            &logger,
        )
        .unwrap();
        assert_eq!(result.required_upgrade_on.unwrap(), KubernetesNodesType::Workers);
        assert_eq!(result.deployed_masters_version, version_1_26);
        assert_eq!(result.deployed_workers_version, version_1_25);
        assert!(!result.older_masters_version_detected); // not true because we're in an upgrade process
        assert!(!result.older_workers_version_detected); // not true because we're in an upgrade process
    }

    #[allow(dead_code)]
    pub fn print_kubernetes_version(provider_version: &VersionsNumber, provider: &VersionsNumber) {
        println!(
            "Provider version: {} | Wished version: {} | Is upgrade required: {:?}",
            provider_version.clone(),
            provider.clone(),
            compare_kubernetes_cluster_versions_for_upgrade(provider_version, provider)
                .unwrap()
                .message
        )
    }

    #[test]
    pub fn check_kubernetes_master_versions() {
        struct KubernetesVersionToCheck {
            json: &'static str,
            wished_version: VersionsNumber,
        }

        let kubectl_version_aws = r#"
{
  "clientVersion": {
    "major": "1",
    "minor": "21",
    "gitVersion": "v1.21.0",
    "gitCommit": "cb303e613a121a29364f75cc67d3d580833a7479",
    "gitTreeState": "archive",
    "buildDate": "2021-04-09T16:47:30Z",
    "goVersion": "go1.16.3",
    "compiler": "gc",
    "platform": "linux/amd64"
  },
  "serverVersion": {
    "major": "1",
    "minor": "16+",
    "gitVersion": "v1.16.15-eks-ad4801",
    "gitCommit": "ad4801fd44fe0f125c8d13f1b1d4827e8884476d",
    "gitTreeState": "clean",
    "buildDate": "2020-10-20T23:27:12Z",
    "goVersion": "go1.13.15",
    "compiler": "gc",
    "platform": "linux/amd64"
  }
}
"#;
        let kubectl_version_do = r#"
        {
  "clientVersion": {
    "major": "1",
    "minor": "21",
    "gitVersion": "v1.21.0",
    "gitCommit": "cb303e613a121a29364f75cc67d3d580833a7479",
    "gitTreeState": "archive",
    "buildDate": "2021-04-09T16:47:30Z",
    "goVersion": "go1.16.3",
    "compiler": "gc",
    "platform": "linux/amd64"
  },
  "serverVersion": {
    "major": "1",
    "minor": "18",
    "gitVersion": "v1.18.10",
    "gitCommit": "62876fc6d93e891aa7fbe19771e6a6c03773b0f7",
    "gitTreeState": "clean",
    "buildDate": "2020-10-15T01:43:56Z",
    "goVersion": "go1.13.15",
    "compiler": "gc",
    "platform": "linux/amd64"
  }
}
"#;

        let validate_providers = vec![
            KubernetesVersionToCheck {
                json: kubectl_version_aws,
                wished_version: VersionsNumber::new("1".to_string(), Some("16".to_string()), None, None),
            },
            KubernetesVersionToCheck {
                json: kubectl_version_do,
                wished_version: VersionsNumber::new("1".to_string(), Some("18".to_string()), None, None),
            },
        ];

        for mut provider in validate_providers {
            let provider_server_version: KubernetesVersion = serde_json::from_str(provider.json).unwrap();
            let provider_version = VersionsNumber::from_str(
                format!(
                    "{}",
                    VersionsNumber::new(
                        provider_server_version.server_version.major,
                        Some(provider_server_version.server_version.minor),
                        None,
                        None,
                    ),
                )
                .as_str(),
            )
            .expect("wrong kubernetes cluster version");

            // upgrade is not required
            //print_kubernetes_version(&provider_version, &provider.wished_version);
            assert!(
                !compare_kubernetes_cluster_versions_for_upgrade(&provider_version, &provider.wished_version)
                    .unwrap()
                    .upgraded_required,
            );

            // upgrade is required
            let add_one_version = provider.wished_version.minor.unwrap().parse::<i32>().unwrap() + 1;
            provider.wished_version.minor = Some(add_one_version.to_string());
            //print_kubernetes_version(&provider_version, &provider.wished_version);
            assert!(
                compare_kubernetes_cluster_versions_for_upgrade(&provider_version, &provider.wished_version)
                    .unwrap()
                    .upgraded_required
            )
        }
    }

    #[test]
    pub fn check_kubernetes_workers_versions() {
        struct KubernetesVersionToCheck {
            json: &'static str,
            wished_version: VersionsNumber,
        }

        let kubectl_version_aws = r#"
{
    "apiVersion": "v1",
    "items": [
        {
            "apiVersion": "v1",
            "kind": "Node",
            "metadata": {
                "annotations": {
                    "node.alpha.kubernetes.io/ttl": "0",
                    "volumes.kubernetes.io/controller-managed-attach-detach": "true"
                },
                "creationTimestamp": "2021-04-30T07:23:17Z",
                "labels": {
                    "beta.kubernetes.io/arch": "amd64",
                    "beta.kubernetes.io/instance-type": "t2.large",
                    "beta.kubernetes.io/os": "linux",
                    "eks.amazonaws.com/nodegroup": "qovery-dmubm9agk7sr8a8r-1",
                    "eks.amazonaws.com/nodegroup-image": "ami-0f8d6052f6e3a19d2",
                    "failure-domain.beta.kubernetes.io/region": "us-east-2",
                    "failure-domain.beta.kubernetes.io/zone": "us-east-2c",
                    "kubernetes.io/arch": "amd64",
                    "kubernetes.io/hostname": "ip-10-0-105-29.us-east-2.compute.internal",
                    "kubernetes.io/os": "linux"
                },
                "name": "ip-10-0-105-29.us-east-2.compute.internal",
                "resourceVersion": "76995588",
                "selfLink": "/api/v1/nodes/ip-10-0-105-29.us-east-2.compute.internal",
                "uid": "dbe8d9e1-481a-4de5-9fa5-1c0b2f2e94e9"
            },
            "spec": {
                "providerID": "aws:///us-east-2c/i-0a99d3bb7b27d62ac"
            },
            "status": {
                "addresses": [
                    {
                        "address": "10.0.105.29",
                        "type": "InternalIP"
                    },
                    {
                        "address": "3.139.58.222",
                        "type": "ExternalIP"
                    },
                    {
                        "address": "ip-10-0-105-29.us-east-2.compute.internal",
                        "type": "Hostname"
                    },
                    {
                        "address": "ip-10-0-105-29.us-east-2.compute.internal",
                        "type": "InternalDNS"
                    },
                    {
                        "address": "ec2-3-139-58-222.us-east-2.compute.amazonaws.com",
                        "type": "ExternalDNS"
                    }
                ],
                "allocatable": {
                    "attachable-volumes-aws-ebs": "39",
                    "cpu": "1930m",
                    "ephemeral-storage": "18242267924",
                    "hugepages-2Mi": "0",
                    "memory": "7408576Ki",
                    "pods": "35"
                },
                "capacity": {
                    "attachable-volumes-aws-ebs": "39",
                    "cpu": "2",
                    "ephemeral-storage": "20959212Ki",
                    "hugepages-2Mi": "0",
                    "memory": "8166336Ki",
                    "pods": "35"
                },
                "conditions": [
                    {
                        "lastHeartbeatTime": "2021-05-13T13:45:52Z",
                        "lastTransitionTime": "2021-04-30T07:23:16Z",
                        "message": "kubelet has sufficient memory available",
                        "reason": "KubeletHasSufficientMemory",
                        "status": "False",
                        "type": "MemoryPressure"
                    },
                    {
                        "lastHeartbeatTime": "2021-05-13T13:45:52Z",
                        "lastTransitionTime": "2021-04-30T07:23:16Z",
                        "message": "kubelet has no disk pressure",
                        "reason": "KubeletHasNoDiskPressure",
                        "status": "False",
                        "type": "DiskPressure"
                    },
                    {
                        "lastHeartbeatTime": "2021-05-13T13:45:52Z",
                        "lastTransitionTime": "2021-04-30T07:23:16Z",
                        "message": "kubelet has sufficient PID available",
                        "reason": "KubeletHasSufficientPID",
                        "status": "False",
                        "type": "PIDPressure"
                    },
                    {
                        "lastHeartbeatTime": "2021-05-13T13:45:52Z",
                        "lastTransitionTime": "2021-04-30T07:23:58Z",
                        "message": "kubelet is posting ready status",
                        "reason": "KubeletReady",
                        "status": "True",
                        "type": "Ready"
                    }
                ],
                "daemonEndpoints": {
                    "kubeletEndpoint": {
                        "Port": 10250
                    }
                },
                "images": [
                    {
                        "names": [
                            "grafana/loki@sha256:72fdf006e78141aa1f449acdbbaa195d4b7ad6be559a6710e4bcfe5ea2d7cc80",
                            "grafana/loki:1.6.0"
                        ],
                        "sizeBytes": 72825761
                    }
                ],
                "nodeInfo": {
                    "architecture": "amd64",
                    "bootID": "6707bff0-c846-4ae5-971f-6213a09cbb8d",
                    "containerRuntimeVersion": "docker://19.3.6",
                    "kernelVersion": "4.14.198-152.320.amzn2.x86_64",
                    "kubeProxyVersion": "v1.16.13-eks-ec92d4",
                    "kubeletVersion": "v1.16.13-eks-ec92d4",
                    "machineID": "9e41586f1a7b461a8987a1110da45b2a",
                    "operatingSystem": "linux",
                    "osImage": "Amazon Linux 2",
                    "systemUUID": "EC2E8B4C-92F9-213B-09B5-C0CD11A7EEB7"
                }
            }
        }
    ],
    "kind": "List",
    "metadata": {
        "resourceVersion": "",
        "selfLink": ""
    }
}
"#;

        let validate_providers = vec![KubernetesVersionToCheck {
            json: kubectl_version_aws,
            wished_version: VersionsNumber::new("1".to_string(), Some("16".to_string()), None, None),
        }];

        for mut provider in validate_providers {
            let provider_server_version: KubernetesList<KubernetesNode> =
                serde_json::from_str(provider.json).expect("Can't read workers json from {} provider");
            for node in provider_server_version.items {
                let kubelet = VersionsNumber::from_str(&node.status.node_info.kubelet_version).unwrap();
                let kube_proxy = VersionsNumber::from_str(&node.status.node_info.kube_proxy_version).unwrap();

                // upgrade is not required
                //print_kubernetes_version(&provider_version, &provider.wished_version);
                assert!(
                    !compare_kubernetes_cluster_versions_for_upgrade(&kubelet, &provider.wished_version)
                        .unwrap()
                        .upgraded_required,
                );
                assert!(
                    !compare_kubernetes_cluster_versions_for_upgrade(&kube_proxy, &provider.wished_version)
                        .unwrap()
                        .upgraded_required,
                );

                // upgrade is required
                let kubelet_add_one_version =
                    provider.wished_version.minor.clone().unwrap().parse::<i32>().unwrap() + 1;
                provider.wished_version.minor = Some(kubelet_add_one_version.to_string());
                //print_kubernetes_version(&provider_version, &provider.wished_version);
                assert!(
                    compare_kubernetes_cluster_versions_for_upgrade(&kubelet, &provider.wished_version)
                        .unwrap()
                        .upgraded_required
                );
            }
        }
    }

    #[test]
    pub fn test_k8s_milli_cpu_convert() {
        let milli_cpu = "250m".to_string();
        let int_cpu = "2".to_string();

        assert_eq!(convert_k8s_cpu_value_to_f32(milli_cpu).unwrap(), 0.25_f32);
        assert_eq!(convert_k8s_cpu_value_to_f32(int_cpu).unwrap(), 2_f32);
    }

    #[test]
    pub fn test_cpu_set() {
        let mut total_cpu = "0.25".to_string();
        let mut cpu_burst = "1".to_string();
        assert_eq!(
            validate_k8s_required_cpu_and_burstable(total_cpu, cpu_burst).unwrap(),
            CpuLimits {
                cpu_request: "0.25".to_string(),
                cpu_limit: "1".to_string()
            }
        );

        total_cpu = "1".to_string();
        cpu_burst = "0.5".to_string();
        assert_eq!(
            validate_k8s_required_cpu_and_burstable(total_cpu, cpu_burst).unwrap(),
            CpuLimits {
                cpu_request: "1".to_string(),
                cpu_limit: "1".to_string()
            }
        );
    }

    #[test]
    pub fn test_kubernetes_version_from_string() {
        // EKS / Kapsule / GKE
        for k8s_version_str in K8sVersion::iter().map(|v| v.to_string()) {
            assert_eq!(
                match k8s_version_str.as_str() {
                    "1.23" => Ok(kubernetes::KubernetesVersion::V1_23 {
                        prefix: None,
                        patch: None,
                        suffix: None,
                    }),
                    "1.24" => Ok(kubernetes::KubernetesVersion::V1_24 {
                        prefix: None,
                        patch: None,
                        suffix: None,
                    }),
                    "1.25" => Ok(kubernetes::KubernetesVersion::V1_25 {
                        prefix: None,
                        patch: None,
                        suffix: None,
                    }),
                    "1.26" => Ok(kubernetes::KubernetesVersion::V1_26 {
                        prefix: None,
                        patch: None,
                        suffix: None,
                    }),
                    "1.27" => Ok(kubernetes::KubernetesVersion::V1_27 {
                        prefix: None,
                        patch: None,
                        suffix: None,
                    }),
                    "1.28" => Ok(kubernetes::KubernetesVersion::V1_28 {
                        prefix: None,
                        patch: None,
                        suffix: None,
                    }),
                    _ => panic!("unsupported k8s version string"),
                },
                K8sVersion::from_str(&k8s_version_str)
            );
        }

        // K3S
        for k3s_versions in [
            "v1.23.16+k3s1",
            "v1.24.14+k3s1",
            "v1.25.11+k3s1",
            "v1.26.6+k3s1",
            "v1.27.9+k3s1",
        ] {
            assert_eq!(
                match k3s_versions {
                    "v1.23.16+k3s1" => Ok(kubernetes::KubernetesVersion::V1_23 {
                        prefix: Some(Arc::from("v")),
                        patch: Some(16),
                        suffix: Some(Arc::from("+k3s1")),
                    }),
                    "v1.24.14+k3s1" => Ok(kubernetes::KubernetesVersion::V1_24 {
                        prefix: Some(Arc::from("v")),
                        patch: Some(14),
                        suffix: Some(Arc::from("+k3s1")),
                    }),
                    "v1.25.11+k3s1" => Ok(kubernetes::KubernetesVersion::V1_25 {
                        prefix: Some(Arc::from("v")),
                        patch: Some(11),
                        suffix: Some(Arc::from("+k3s1")),
                    }),
                    "v1.26.6+k3s1" => Ok(kubernetes::KubernetesVersion::V1_26 {
                        prefix: Some(Arc::from("v")),
                        patch: Some(6),
                        suffix: Some(Arc::from("+k3s1")),
                    }),
                    "v1.27.9+k3s1" => Ok(kubernetes::KubernetesVersion::V1_27 {
                        prefix: Some(Arc::from("v")),
                        patch: Some(9),
                        suffix: Some(Arc::from("+k3s1")),
                    }),
                    _ => panic!("unsupported k3s version string"),
                },
                K8sVersion::from_str(k3s_versions)
            );
        }

        // failing tests
        assert!(K8sVersion::from_str("toto").is_err());
    }

    #[test]
    pub fn test_kubernetes_version_into_version_number() {
        // EKS / Kapsule / GKE
        for k8s_version in K8sVersion::iter() {
            assert_eq!(
                VersionsNumber::new(
                    k8s_version.major().to_string(),
                    Some(k8s_version.minor().to_string()),
                    None,
                    None
                ),
                VersionsNumber::from(k8s_version)
            );
        }

        // K3S
        for k3s_version_str in [
            "v1.23.16+k3s1",
            "v1.24.14+k3s1",
            "v1.25.11+k3s1",
            "v1.26.6+k3s1",
            "v1.27.9+k3s1",
        ] {
            let k3s_version = K8sVersion::from_str(k3s_version_str).expect("Unknown k3s string version");
            assert_eq!(
                VersionsNumber::new(
                    k3s_version.major().to_string(),
                    Some(k3s_version.minor().to_string()),
                    k3s_version.patch().as_ref().map(|p| p.to_string()),
                    k3s_version.suffix().as_ref().map(|s| s.to_string()),
                ),
                VersionsNumber::from(k3s_version),
            );
        }
    }

    #[test]
    pub fn test_kubernetes_version_next_version() {
        // EKS / Kapsule / GCP
        for k8s_version in K8sVersion::iter() {
            assert_eq!(
                match k8s_version {
                    K8sVersion::V1_23 { .. } => Some(K8sVersion::V1_24 {
                        prefix: None,
                        patch: None,
                        suffix: None
                    }),
                    K8sVersion::V1_24 { .. } => Some(K8sVersion::V1_25 {
                        prefix: None,
                        patch: None,
                        suffix: None
                    }),
                    K8sVersion::V1_25 { .. } => Some(K8sVersion::V1_26 {
                        prefix: None,
                        patch: None,
                        suffix: None
                    }),
                    K8sVersion::V1_26 { .. } => Some(K8sVersion::V1_27 {
                        prefix: None,
                        patch: None,
                        suffix: None
                    }),
                    K8sVersion::V1_27 { .. } => Some(K8sVersion::V1_28 {
                        prefix: None,
                        patch: None,
                        suffix: None
                    }),
                    K8sVersion::V1_28 { .. } => None,
                },
                k8s_version.next_version(),
            );
        }

        // K3S
        for k3s_version_str in [
            "v1.23.16+k3s1",
            "v1.24.14+k3s1",
            "v1.25.11+k3s1",
            "v1.26.6+k3s1",
            "v1.27.9+k3s1",
        ] {
            let k3s_version = K8sVersion::from_str(k3s_version_str).expect("Unknown k3s string version");
            assert_eq!(
                match k3s_version {
                    K8sVersion::V1_23 { .. } => Some(K8sVersion::V1_24 {
                        prefix: None,
                        patch: None,
                        suffix: None
                    }),
                    K8sVersion::V1_24 { .. } => Some(K8sVersion::V1_25 {
                        prefix: None,
                        patch: None,
                        suffix: None
                    }),
                    K8sVersion::V1_25 { .. } => Some(K8sVersion::V1_26 {
                        prefix: None,
                        patch: None,
                        suffix: None
                    }),
                    K8sVersion::V1_26 { .. } => Some(K8sVersion::V1_27 {
                        prefix: None,
                        patch: None,
                        suffix: None
                    }),
                    K8sVersion::V1_27 { .. } => Some(K8sVersion::V1_28 {
                        prefix: None,
                        patch: None,
                        suffix: None
                    }),
                    K8sVersion::V1_28 { .. } => None,
                },
                k3s_version.next_version(),
            );
        }
    }

    #[test]
    pub fn test_kubernetes_version_functions() {
        let version_1_23 = K8sVersion::V1_23 {
            prefix: None,
            patch: None,
            suffix: None,
        };
        assert_eq!(version_1_23.prefix().clone(), None);
        assert_eq!(version_1_23.major(), 1);
        assert_eq!(version_1_23.minor(), 23);
        assert_eq!(version_1_23.patch().clone(), None);
        assert_eq!(version_1_23.suffix().clone(), None);
        assert_eq!(version_1_23.to_string(), "1.23".to_string());

        let version_full = K8sVersion::V1_24 {
            prefix: Some(Arc::from("v")),
            patch: Some(16),
            suffix: Some(Arc::from("+k3s1")),
        };
        assert_eq!(
            version_full.prefix().clone().expect("Unable to get version's prefix"),
            Arc::from("v"),
        );
        assert_eq!(version_full.major(), 1);
        assert_eq!(version_full.minor(), 24);
        assert_eq!(version_full.patch().expect("Unable to get version's patch"), 16);
        assert_eq!(
            version_full.suffix().clone().expect("Unable to get version's suffix"),
            Arc::from("+k3s1")
        );
        assert_eq!(version_full.to_string(), "v1.24.16+k3s1".to_string())
    }
}

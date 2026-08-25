mod vsphere;

use crate::errors::CommandError;
use crate::infrastructure::action::InfraLogger;
use crate::infrastructure::models::cloud_provider::CloudProvider;
use serde::Deserialize;
use serde_yaml::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EksAnywhereProviderMode {
    VSphere,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ProviderPreflightError {
    VSphereCloudCredentialsRejected(CommandError),
    Other(CommandError),
}

impl ProviderPreflightError {
    /// Returns the user-facing message associated with this classified preflight failure.
    pub fn user_message(&self) -> &'static str {
        match self {
            Self::VSphereCloudCredentialsRejected(_) => VSPHERE_AUTHENTICATION_ERROR_MESSAGE,
            Self::Other(_) => VSPHERE_PREFLIGHT_ERROR_MESSAGE,
        }
    }
}

impl From<CommandError> for ProviderPreflightError {
    fn from(error: CommandError) -> Self {
        Self::Other(error)
    }
}

const VSPHERE_AUTHENTICATION_ERROR_MESSAGE: &str = "vSphere authentication failed: vCenter rejected the vSphere cloud credentials configured for this cluster. Verify the associated cloud credentials and retry.";
const VSPHERE_PREFLIGHT_ERROR_MESSAGE: &str = "vSphere preflight checks failed";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct ParsedEksAnywhereClusterConfig {
    documents: Vec<EksAnywhereConfigDocument>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum EksAnywhereConfigDocument {
    Cluster(ParsedClusterSpec),
    VSphereDatacenterConfig(ParsedVSphereDatacenterConfig),
    VSphereMachineConfig(ParsedVSphereMachineConfig),
    Other(ParsedUnknownDocument),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct ParsedClusterSpec {
    cluster_name: Option<EksAnywhereClusterName>,
    pub kubernetes_version: Option<EksAnywhereKubernetesVersion>,
    machine_group_targets: Vec<ParsedMachineGroupTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct EksAnywhereKubernetesVersion(String);

impl EksAnywhereKubernetesVersion {
    fn from_config(value: &str) -> Option<Self> {
        let value = value.trim();
        (!value.is_empty()).then(|| Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedMachineGroupTarget {
    machine_config_name: String,
    kubernetes_version: EksAnywhereKubernetesVersion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EksAnywhereClusterName(String);

impl EksAnywhereClusterName {
    fn from_config(value: &str) -> Option<Self> {
        let value = value.trim();
        (!value.is_empty()).then(|| Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct ParsedVSphereDatacenterConfig {
    pub server: Option<String>,
    pub insecure: Option<bool>,
    pub network: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct ParsedVSphereMachineConfig {
    pub name: String,
    pub template: Option<String>,
    pub os_family: Option<String>,
    pub datastore: Option<String>,
    pub resource_pool: Option<String>,
    pub folder: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct ParsedUnknownDocument {
    pub kind: Option<String>,
}

impl ParsedEksAnywhereClusterConfig {
    pub fn provider_mode(&self) -> EksAnywhereProviderMode {
        if self.documents.iter().any(|doc| {
            matches!(
                doc,
                EksAnywhereConfigDocument::VSphereDatacenterConfig(_)
                    | EksAnywhereConfigDocument::VSphereMachineConfig(_)
            )
        }) {
            return EksAnywhereProviderMode::VSphere;
        }

        EksAnywhereProviderMode::Unknown
    }

    pub fn cluster_spec(&self) -> Option<&ParsedClusterSpec> {
        self.documents.iter().find_map(|doc| match doc {
            EksAnywhereConfigDocument::Cluster(spec) => Some(spec),
            _ => None,
        })
    }

    pub fn cluster_name(&self) -> Option<&EksAnywhereClusterName> {
        self.cluster_spec()?.cluster_name.as_ref()
    }

    pub fn vsphere_datacenter_config(&self) -> Option<&ParsedVSphereDatacenterConfig> {
        self.documents.iter().find_map(|doc| match doc {
            EksAnywhereConfigDocument::VSphereDatacenterConfig(config) => Some(config),
            _ => None,
        })
    }

    pub fn vsphere_machine_configs(&self) -> impl Iterator<Item = &ParsedVSphereMachineConfig> {
        self.documents.iter().filter_map(|doc| match doc {
            EksAnywhereConfigDocument::VSphereMachineConfig(config) => Some(config),
            _ => None,
        })
    }

    pub fn effective_kubernetes_versions_for_machine_config(
        &self,
        machine_config_name: &str,
    ) -> BTreeSet<EksAnywhereKubernetesVersion> {
        self.cluster_spec()
            .into_iter()
            .flat_map(|cluster| cluster.machine_group_targets.iter())
            .filter(|target| target.machine_config_name == machine_config_name)
            .map(|target| target.kubernetes_version.clone())
            .collect()
    }
}

fn parse_machine_group_ref_name(value: &Value) -> Option<String> {
    value
        .get("machineGroupRef")
        .and_then(|machine_group_ref| machine_group_ref.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

fn parse_cluster_spec(value: &Value) -> ParsedClusterSpec {
    let spec = value.get("spec");
    let kubernetes_version = spec
        .and_then(|spec| spec.get("kubernetesVersion"))
        .and_then(Value::as_str)
        .and_then(EksAnywhereKubernetesVersion::from_config);
    let mut machine_group_targets = Vec::new();

    if let (Some(spec), Some(kubernetes_version)) = (spec, kubernetes_version.as_ref()) {
        for configuration_name in ["controlPlaneConfiguration", "externalEtcdConfiguration"] {
            if let Some(machine_config_name) = spec.get(configuration_name).and_then(parse_machine_group_ref_name) {
                machine_group_targets.push(ParsedMachineGroupTarget {
                    machine_config_name,
                    kubernetes_version: kubernetes_version.clone(),
                });
            }
        }
    }

    if let Some(worker_node_groups) = spec
        .and_then(|spec| spec.get("workerNodeGroupConfigurations"))
        .and_then(Value::as_sequence)
    {
        for worker_node_group in worker_node_groups {
            let Some(machine_config_name) = parse_machine_group_ref_name(worker_node_group) else {
                continue;
            };
            let worker_kubernetes_version = worker_node_group
                .get("kubernetesVersion")
                .and_then(Value::as_str)
                .and_then(EksAnywhereKubernetesVersion::from_config)
                .or_else(|| kubernetes_version.clone());
            let Some(worker_kubernetes_version) = worker_kubernetes_version else {
                continue;
            };

            machine_group_targets.push(ParsedMachineGroupTarget {
                machine_config_name,
                kubernetes_version: worker_kubernetes_version,
            });
        }
    }

    ParsedClusterSpec {
        cluster_name: value
            .get("metadata")
            .and_then(|metadata| metadata.get("name"))
            .and_then(Value::as_str)
            .and_then(EksAnywhereClusterName::from_config),
        kubernetes_version,
        machine_group_targets,
    }
}

pub(super) fn parse_eks_anywhere_cluster_config(
    cluster_config_path: &Path,
) -> Result<ParsedEksAnywhereClusterConfig, CommandError> {
    let content = fs::read_to_string(cluster_config_path).map_err(|e| {
        CommandError::new(
            format!("Cannot read cluster config file {}", cluster_config_path.display()),
            Some(e.to_string()),
            None,
        )
    })?;

    parse_eks_anywhere_cluster_config_from_yaml(&content)
}

fn parse_eks_anywhere_cluster_config_from_yaml(content: &str) -> Result<ParsedEksAnywhereClusterConfig, CommandError> {
    let mut documents = Vec::new();

    for yaml_doc in serde_yaml::Deserializer::from_str(content) {
        let value = Value::deserialize(yaml_doc).map_err(|e| {
            CommandError::new(
                "Cannot parse EKS Anywhere cluster YAML while detecting provider mode".to_string(),
                Some(e.to_string()),
                None,
            )
        })?;

        let kind = value.get("kind").and_then(Value::as_str);
        match kind {
            Some("Cluster") => {
                documents.push(EksAnywhereConfigDocument::Cluster(parse_cluster_spec(&value)));
            }
            Some("VSphereDatacenterConfig") => {
                let spec = ParsedVSphereDatacenterConfig {
                    server: value
                        .get("spec")
                        .and_then(|s| s.get("server"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    insecure: value
                        .get("spec")
                        .and_then(|s| s.get("insecure"))
                        .and_then(Value::as_bool),
                    network: value
                        .get("spec")
                        .and_then(|s| s.get("network"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                };
                documents.push(EksAnywhereConfigDocument::VSphereDatacenterConfig(spec));
            }
            Some("VSphereMachineConfig") => {
                let spec = ParsedVSphereMachineConfig {
                    name: value
                        .get("metadata")
                        .and_then(|m| m.get("name"))
                        .and_then(Value::as_str)
                        .unwrap_or("<unknown>")
                        .to_string(),
                    template: value
                        .get("spec")
                        .and_then(|s| s.get("template"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    os_family: value
                        .get("spec")
                        .and_then(|s| s.get("osFamily"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    datastore: value
                        .get("spec")
                        .and_then(|s| s.get("datastore"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    resource_pool: value
                        .get("spec")
                        .and_then(|s| s.get("resourcePool"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    folder: value
                        .get("spec")
                        .and_then(|s| s.get("folder"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                };
                documents.push(EksAnywhereConfigDocument::VSphereMachineConfig(spec));
            }
            _ => {
                documents.push(EksAnywhereConfigDocument::Other(ParsedUnknownDocument {
                    kind: kind.map(str::to_string),
                }));
            }
        }
    }

    Ok(ParsedEksAnywhereClusterConfig { documents })
}

pub(super) fn run_provider_preflight_for_mode(
    provider_mode: EksAnywhereProviderMode,
    parsed_cluster_config: &ParsedEksAnywhereClusterConfig,
    cluster_config_path: &Path,
    cloud_provider: &dyn CloudProvider,
    install_missing: bool,
    expected_eksd_release_tag: Option<&str>,
    logger: &impl InfraLogger,
) -> Result<(), ProviderPreflightError> {
    match provider_mode {
        EksAnywhereProviderMode::VSphere => vsphere::run_vsphere_preflight(
            parsed_cluster_config,
            cluster_config_path,
            cloud_provider,
            install_missing,
            expected_eksd_release_tag,
            logger,
        ),
        EksAnywhereProviderMode::Unknown => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::{EksAnywhereProviderMode, ProviderPreflightError, parse_eks_anywhere_cluster_config_from_yaml};
    use crate::errors::CommandError;

    #[test]
    fn should_detect_vsphere_provider_mode() {
        let yaml = r#"
kind: Cluster
metadata:
  name: my-cluster
---
kind: VSphereMachineConfig
metadata:
  name: cp-machine
spec:
  template: my-template
"#;

        let mode = parse_eks_anywhere_cluster_config_from_yaml(yaml)
            .expect("YAML should parse")
            .provider_mode();
        assert_eq!(mode, EksAnywhereProviderMode::VSphere);

        let parsed = parse_eks_anywhere_cluster_config_from_yaml(yaml).expect("YAML should parse");
        assert_eq!(parsed.cluster_name().map(|name| name.as_str()), Some("my-cluster"));
        assert_eq!(parsed.vsphere_machine_configs().count(), 1);
        assert!(parsed.vsphere_datacenter_config().is_none());
    }

    #[test]
    fn should_fallback_to_unknown_provider_mode() {
        let yaml = r#"
kind: Cluster
metadata:
  name: my-cluster
---
kind: DockerDatacenterConfig
metadata:
  name: dc
"#;

        let mode = parse_eks_anywhere_cluster_config_from_yaml(yaml)
            .expect("YAML should parse")
            .provider_mode();
        assert_eq!(mode, EksAnywhereProviderMode::Unknown);
    }

    #[test]
    fn should_expose_rejected_vsphere_cloud_credentials_to_user() {
        let error = ProviderPreflightError::VSphereCloudCredentialsRejected(CommandError::new(
            "vSphere cloud credentials rejected".to_string(),
            Some("Structured SOAP InvalidLogin fault".to_string()),
            None,
        ));

        assert_eq!(
            error.user_message(),
            "vSphere authentication failed: vCenter rejected the vSphere cloud credentials configured for this cluster. Verify the associated cloud credentials and retry."
        );
    }

    #[test]
    fn should_keep_generic_provider_preflight_messages_for_other_errors() {
        let error =
            ProviderPreflightError::Other(CommandError::new_from_safe_message("Template not found".to_string()));

        assert_eq!(error.user_message(), "vSphere preflight checks failed");
    }
}

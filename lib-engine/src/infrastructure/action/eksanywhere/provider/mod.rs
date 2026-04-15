mod vsphere;

use crate::errors::CommandError;
use crate::infrastructure::action::InfraLogger;
use crate::infrastructure::models::cloud_provider::CloudProvider;
use serde::Deserialize;
use serde_yaml::Value;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EksAnywhereProviderMode {
    VSphere,
    Unknown,
}

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
    pub kubernetes_version: Option<String>,
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
                let spec = ParsedClusterSpec {
                    kubernetes_version: value
                        .get("spec")
                        .and_then(|s| s.get("kubernetesVersion"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                };
                documents.push(EksAnywhereConfigDocument::Cluster(spec));
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
) -> Result<(), CommandError> {
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
    use super::{EksAnywhereProviderMode, parse_eks_anywhere_cluster_config_from_yaml};

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
}

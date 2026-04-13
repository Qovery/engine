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

pub(super) fn detect_provider_mode_from_cluster_config(
    cluster_config_path: &Path,
) -> Result<EksAnywhereProviderMode, CommandError> {
    let content = fs::read_to_string(cluster_config_path).map_err(|e| {
        CommandError::new(
            format!("Cannot read cluster config file {}", cluster_config_path.display()),
            Some(e.to_string()),
            None,
        )
    })?;

    detect_provider_mode_from_yaml(&content)
}

fn detect_provider_mode_from_yaml(content: &str) -> Result<EksAnywhereProviderMode, CommandError> {
    for yaml_doc in serde_yaml::Deserializer::from_str(content) {
        let value = Value::deserialize(yaml_doc).map_err(|e| {
            CommandError::new(
                "Cannot parse EKS Anywhere cluster YAML while detecting provider mode".to_string(),
                Some(e.to_string()),
                None,
            )
        })?;

        let Some(kind) = value.get("kind").and_then(Value::as_str) else {
            continue;
        };

        if kind.starts_with("VSphere") {
            return Ok(EksAnywhereProviderMode::VSphere);
        }
    }

    Ok(EksAnywhereProviderMode::Unknown)
}

pub(super) fn run_provider_preflight_for_mode(
    provider_mode: EksAnywhereProviderMode,
    cluster_config_path: &Path,
    cloud_provider: &dyn CloudProvider,
    install_missing: bool,
    expected_eksd_release_tag: Option<&str>,
    logger: &impl InfraLogger,
) -> Result<(), CommandError> {
    match provider_mode {
        EksAnywhereProviderMode::VSphere => vsphere::run_vsphere_preflight(
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
    use super::{EksAnywhereProviderMode, detect_provider_mode_from_yaml};

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

        let mode = detect_provider_mode_from_yaml(yaml).expect("YAML should parse");
        assert_eq!(mode, EksAnywhereProviderMode::VSphere);
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

        let mode = detect_provider_mode_from_yaml(yaml).expect("YAML should parse");
        assert_eq!(mode, EksAnywhereProviderMode::Unknown);
    }
}

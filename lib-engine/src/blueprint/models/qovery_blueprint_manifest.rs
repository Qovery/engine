// Qovery Blueprint Manifest (QBM)
//
// Parses the qbm.yml file found in a service-catalog blueprint directory.
// Only the fields the engine needs for execution are parsed here.
// Metadata, variables and contextVariables are for the console/q-core layer: q-core resolves the
// context variables against the cluster and sends them with the rest of the variable set.

use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer};
use std::collections::HashMap;
use std::path::Path;

#[derive(Deserialize, Debug, Clone)]
pub struct QoveryBlueprintManifest {
    pub kind: BlueprintKind,
    #[serde(default)]
    pub metadata: BlueprintMetadata,
    pub spec: BlueprintSpec,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct BlueprintMetadata {
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub enum BlueprintKind {
    ServiceBlueprint,
    StackBlueprint,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct BlueprintCredentials {
    #[serde(default)]
    pub default: CredentialMode,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum CredentialMode {
    #[default]
    Cluster,
    Env,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct BlueprintBackend {
    #[serde(default)]
    pub default: BackendMode,
    /// Blueprint backend configuration. Only used when `default` is `Blueprint`.
    #[serde(default)]
    pub user_provided: Option<BlueprintBackendConfig>,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub enum BackendMode {
    #[default]
    #[serde(rename = "qovery")]
    Qovery,
    #[serde(rename = "user_provided")]
    Blueprint,
}

// User-provided terraform backend. Catalog declares the backend type + config (e.g. "s3" with
// bucket/region). The engine forwards this to the Qovery Terraform provider, which generates +
// injects backend.tf for the created service. Credentials live elsewhere (env vars at runtime).
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct BlueprintBackendConfig {
    /// Terraform backend type (e.g. "s3", "gcs", "azurerm").
    #[serde(rename = "type")]
    pub backend_type: String,
    /// Static backend config (bucket, region, etc.).
    #[serde(default)]
    pub config: HashMap<String, String>,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct BlueprintResources {
    pub cpu: Option<String>,
    pub ram: Option<String>,
    #[serde(default)]
    pub storage: Option<String>,
}

/// Intermediate helper: serde deserializes this, then the BlueprintSpec impl routes it into
/// the BlueprintEngine enum.
#[derive(Deserialize)]
struct BlueprintSpecRaw {
    engine: BlueprintEngineConfigRaw,
    #[serde(default)]
    outputs: Vec<BlueprintOutput>,
}

#[derive(Deserialize)]
struct BlueprintEngineConfigRaw {
    #[serde(rename = "type")]
    engine_type: String,
    provider: Option<String>,
    chart: Option<BlueprintChart>,
    /// Engine-specific version block. Present when engine.type=terraform.
    #[serde(default)]
    terraform: Option<EngineVersionBlock>,
    /// Engine-specific version block. Present when engine.type=opentofu.
    #[serde(default)]
    opentofu: Option<EngineVersionBlock>,
    #[serde(default)]
    credentials: Option<BlueprintCredentials>,
    #[serde(default)]
    backend: Option<BlueprintBackend>,
    timeout: Option<u64>,
    #[serde(default)]
    arguments: Vec<String>,
    #[serde(default, rename = "allowClusterWideResources")]
    allow_cluster_wide_resources: bool,
    #[serde(default)]
    resources: Option<BlueprintResources>,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct EngineVersionBlock {
    pub version: String,
}

#[derive(Debug, Clone)]
pub struct BlueprintSpec {
    pub engine: BlueprintEngine,
    pub credentials: BlueprintCredentials,
    pub backend: BlueprintBackend,
    pub timeout: Option<u64>,
    pub arguments: Vec<String>,
    pub allow_cluster_wide_resources: bool,
    pub resources: Option<BlueprintResources>,
    pub engine_version: Option<String>,
}

impl<'de> Deserialize<'de> for BlueprintSpec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = BlueprintSpecRaw::deserialize(deserializer)?;
        let engine_cfg = raw.engine;
        let (engine, engine_version) = match engine_cfg.engine_type.as_str() {
            "terraform" => {
                if engine_cfg.opentofu.is_some() {
                    return Err(D::Error::custom(
                        "'opentofu' block must not be set when engine.type is 'terraform'",
                    ));
                }
                let provider = engine_cfg
                    .provider
                    .ok_or_else(|| D::Error::custom("'provider' is required when engine.type is 'terraform'"))?;
                let block = engine_cfg.terraform.ok_or_else(|| {
                    D::Error::custom("'terraform.version' is required when engine.type is 'terraform'")
                })?;
                let version = block.version;
                if version.is_empty() {
                    return Err(D::Error::custom("'terraform.version' must not be empty"));
                }
                (
                    BlueprintEngine::Terraform {
                        provider,
                        outputs: raw.outputs,
                    },
                    Some(version),
                )
            }
            "opentofu" => {
                if engine_cfg.terraform.is_some() {
                    return Err(D::Error::custom(
                        "'terraform' block must not be set when engine.type is 'opentofu'",
                    ));
                }
                let provider = engine_cfg
                    .provider
                    .ok_or_else(|| D::Error::custom("'provider' is required when engine.type is 'opentofu'"))?;
                let block = engine_cfg
                    .opentofu
                    .ok_or_else(|| D::Error::custom("'opentofu.version' is required when engine.type is 'opentofu'"))?;
                let version = block.version;
                if version.is_empty() {
                    return Err(D::Error::custom("'opentofu.version' must not be empty"));
                }
                (
                    BlueprintEngine::Opentofu {
                        provider,
                        outputs: raw.outputs,
                    },
                    Some(version),
                )
            }
            "helm" => {
                if engine_cfg.terraform.is_some() || engine_cfg.opentofu.is_some() {
                    return Err(D::Error::custom(
                        "'terraform'/'opentofu' blocks must not be set when engine.type is 'helm'",
                    ));
                }
                let chart = engine_cfg
                    .chart
                    .ok_or_else(|| D::Error::custom("'chart' is required when engine.type is 'helm'"))?;
                (
                    BlueprintEngine::Helm {
                        chart,
                        outputs: raw.outputs,
                    },
                    None,
                )
            }
            other => {
                return Err(D::Error::custom(format!("unknown engine.type: '{}'", other)));
            }
        };
        Ok(BlueprintSpec {
            engine,
            credentials: engine_cfg.credentials.unwrap_or_default(),
            backend: engine_cfg.backend.unwrap_or_default(),
            timeout: engine_cfg.timeout,
            arguments: engine_cfg.arguments,
            allow_cluster_wide_resources: engine_cfg.allow_cluster_wide_resources,
            resources: engine_cfg.resources,
            engine_version,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BlueprintEngine {
    Terraform {
        provider: String,
        outputs: Vec<BlueprintOutput>,
    },
    Opentofu {
        provider: String,
        outputs: Vec<BlueprintOutput>,
    },
    Helm {
        chart: BlueprintChart,
        outputs: Vec<BlueprintOutput>,
    },
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct BlueprintChart {
    pub repository: String,
    pub name: String,
    pub version: String,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct BlueprintOutput {
    pub name: String,
    pub description: Option<String>,
    pub sensitive: Option<bool>,
}

impl QoveryBlueprintManifest {
    pub fn parse(path: &Path) -> Result<Self, anyhow::Error> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read QBM file at {}: {}", path.display(), e))?;
        let manifest: QoveryBlueprintManifest = serde_yaml::from_str(&content)
            .map_err(|e| anyhow::anyhow!("Failed to parse QBM YAML at {}: {}", path.display(), e))?;
        Ok(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_terraform_qbm() {
        let yaml = r#"
apiVersion: "qovery.com/v2"
kind: ServiceBlueprint
metadata:
  name: "aws-s3"
  version: "1.0.0"
  serviceFamily: "s3"
spec:
  engine:
    type: terraform
    provider: aws
    terraform:
      version: "1.9.7"
    credentials:
      default: cluster
    timeout: 1800
  contextVariables:
    - name: "region"
      source: "cluster.region"
  outputs:
    - name: bucket_arn
      description: "Bucket ARN"
      sensitive: false
"#;
        let manifest: QoveryBlueprintManifest = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(manifest.kind, BlueprintKind::ServiceBlueprint);
        assert_eq!(manifest.spec.credentials.default, CredentialMode::Cluster);
        assert_eq!(manifest.spec.timeout, Some(1800));
        assert_eq!(manifest.spec.engine_version.as_deref(), Some("1.9.7"));
        let BlueprintEngine::Terraform { provider, outputs } = &manifest.spec.engine else {
            panic!("expected Terraform engine");
        };
        assert_eq!(provider, "aws");
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].name, "bucket_arn");
    }

    #[test]
    fn terraform_without_version_block_fails() {
        let yaml = r#"
apiVersion: "qovery.com/v2"
kind: ServiceBlueprint
metadata:
  name: "bad"
  version: "1.0.0"
spec:
  engine:
    type: terraform
    provider: aws
"#;
        let err = serde_yaml::from_str::<QoveryBlueprintManifest>(yaml)
            .expect_err("expected error when terraform.version is missing");
        assert!(err.to_string().contains("terraform.version"));
    }

    #[test]
    fn terraform_block_on_helm_engine_fails() {
        let yaml = r#"
apiVersion: "qovery.com/v2"
kind: ServiceBlueprint
metadata:
  name: "helm-redis"
  version: "1.0.0"
spec:
  engine:
    type: helm
    chart:
      repository: "https://charts.bitnami.com/bitnami"
      name: "redis"
      version: "20.11.3"
    terraform:
      version: "1.9.7"
"#;
        let err = serde_yaml::from_str::<QoveryBlueprintManifest>(yaml)
            .expect_err("expected error when terraform block is set on helm engine");
        assert!(err.to_string().contains("terraform"));
    }

    #[test]
    fn parse_helm_qbm() {
        let yaml = r#"
apiVersion: "qovery.com/v2"
kind: ServiceBlueprint
metadata:
  name: "helm-redis"
  version: "1.0.0"
spec:
  engine:
    type: helm
    chart:
      repository: "https://charts.bitnami.com/bitnami"
      name: "redis"
      version: "20.11.3"
    arguments: ["--atomic", "--wait"]
    allowClusterWideResources: true
  outputs:
    - name: redis_host
      description: "Redis hostname"
"#;
        let manifest: QoveryBlueprintManifest = serde_yaml::from_str(yaml).unwrap();
        let BlueprintEngine::Helm { chart, outputs } = &manifest.spec.engine else {
            panic!("expected Helm engine");
        };
        assert_eq!(chart.name, "redis");
        assert_eq!(chart.repository, "https://charts.bitnami.com/bitnami");
        assert_eq!(chart.version, "20.11.3");
        assert_eq!(manifest.spec.arguments, vec!["--atomic", "--wait"]);
        assert!(manifest.spec.allow_cluster_wide_resources);
        assert_eq!(outputs.len(), 1);
    }

    #[test]
    fn parse_credentials_env_mode() {
        let yaml = r#"
apiVersion: "qovery.com/v2"
kind: ServiceBlueprint
metadata:
  name: "cross-account"
  version: "1.0.0"
spec:
  engine:
    type: terraform
    provider: aws
    terraform:
      version: "1.9.7"
    credentials:
      default: env
"#;
        let manifest: QoveryBlueprintManifest = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(manifest.spec.credentials.default, CredentialMode::Env);
    }

    #[test]
    fn credentials_default_to_cluster_when_omitted() {
        let yaml = r#"
apiVersion: "qovery.com/v2"
kind: ServiceBlueprint
metadata:
  name: "aws-s3"
  version: "1.0.0"
spec:
  engine:
    type: terraform
    provider: aws
    terraform:
      version: "1.9.7"
"#;
        let manifest: QoveryBlueprintManifest = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(manifest.spec.credentials.default, CredentialMode::Cluster);
    }

    #[test]
    fn parse_stack_blueprint_kind() {
        let yaml = r#"
apiVersion: "qovery.com/v2"
kind: StackBlueprint
metadata:
  name: "my-stack"
  version: "1.0.0"
spec:
  engine:
    type: terraform
    provider: aws
    terraform:
      version: "1.9.7"
"#;
        let manifest: QoveryBlueprintManifest = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(manifest.kind, BlueprintKind::StackBlueprint);
    }

    #[test]
    fn terraform_without_provider_fails() {
        let yaml = r#"
apiVersion: "qovery.com/v2"
kind: ServiceBlueprint
metadata:
  name: "bad"
  version: "1.0.0"
spec:
  engine:
    type: terraform
"#;
        let result: Result<QoveryBlueprintManifest, _> = serde_yaml::from_str(yaml);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("provider"));
    }

    #[test]
    fn helm_without_chart_fails() {
        let yaml = r#"
apiVersion: "qovery.com/v2"
kind: ServiceBlueprint
metadata:
  name: "bad"
  version: "1.0.0"
spec:
  engine:
    type: helm
"#;
        let result: Result<QoveryBlueprintManifest, _> = serde_yaml::from_str(yaml);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("chart"));
    }

    #[test]
    fn unknown_engine_fails() {
        let yaml = r#"
apiVersion: "qovery.com/v2"
kind: ServiceBlueprint
metadata:
  name: "bad"
  version: "1.0.0"
spec:
  engine:
    type: pulumi
"#;
        let result: Result<QoveryBlueprintManifest, _> = serde_yaml::from_str(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn unknown_fields_are_silently_ignored() {
        let yaml = r#"
apiVersion: "qovery.com/v2"
kind: ServiceBlueprint
metadata:
  name: "test"
  version: "1.0.0"
  serviceFamily: "postgres"
  some_future_field: "value"
spec:
  engine:
    type: terraform
    provider: gcp
    terraform:
      version: "1.9.7"
  contextVariables:
    - name: "region"
      source: "cluster.region"
  variables:
    - name: "bucket"
      type: "string"
  some_future_spec_field: 42
"#;
        let manifest: QoveryBlueprintManifest = serde_yaml::from_str(yaml).unwrap();
        let BlueprintEngine::Terraform { provider, .. } = &manifest.spec.engine else {
            panic!("expected Terraform");
        };
        assert_eq!(provider, "gcp");
    }

    #[test]
    fn defaults_when_optional_fields_omitted() {
        let yaml = r#"
apiVersion: "qovery.com/v2"
kind: ServiceBlueprint
metadata:
  name: "minimal"
  version: "1.0.0"
spec:
  engine:
    type: terraform
    provider: aws
    terraform:
      version: "1.9.7"
"#;
        let manifest: QoveryBlueprintManifest = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(manifest.spec.credentials.default, CredentialMode::Cluster);
        assert!(manifest.spec.timeout.is_none());
        assert!(manifest.spec.arguments.is_empty());
        assert!(!manifest.spec.allow_cluster_wide_resources);
        assert_eq!(manifest.spec.engine_version.as_deref(), Some("1.9.7"));
        assert!(manifest.metadata.description.is_none());
    }

    #[test]
    fn parses_metadata_description() {
        let yaml = r#"
apiVersion: "qovery.com/v2"
kind: ServiceBlueprint
metadata:
  name: "aws-s3"
  version: "1.0.1"
  description: "S3 bucket with encryption, versioning, and lifecycle rules"
spec:
  engine:
    type: terraform
    provider: aws
    terraform:
      version: "1.9.7"
"#;
        let manifest: QoveryBlueprintManifest = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            manifest.metadata.description.as_deref(),
            Some("S3 bucket with encryption, versioning, and lifecycle rules"),
        );
    }
}

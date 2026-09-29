// Qovery Blueprint Manifest (QBM)
//
// Parses the qbm.yml file found in a service-catalog blueprint directory.
// Only the fields the engine needs for execution are parsed here.
// Metadata, variables and contextVariables are for the console/q-core layer: q-core resolves the
// context variables against the cluster and sends them with the rest of the variable set.

use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{HashMap, HashSet};
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
    /// Ports the created Helm service exposes. Helm only.
    #[serde(default)]
    ports: Vec<BlueprintHelmPort>,
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
        if !engine_cfg.ports.is_empty() && engine_cfg.engine_type != "helm" {
            return Err(D::Error::custom("'ports' is only supported when engine.type is 'helm'"));
        }
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
                validate_helm_ports(&engine_cfg.ports).map_err(D::Error::custom)?;
                (
                    BlueprintEngine::Helm {
                        chart,
                        outputs: raw.outputs,
                        ports: engine_cfg.ports,
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
        ports: Vec<BlueprintHelmPort>,
    },
}

/// A port of the created Helm service, mapped 1:1 onto `qovery_helm.ports`. Qovery exposes every
/// Helm service port publicly, so declaring one is what gives the blueprint a public URL.
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BlueprintHelmPort {
    pub name: String,
    /// Kubernetes Service the port targets. The chart must give it a fixed name
    /// (fullnameOverride): release names are derived from the Qovery service id.
    pub service_name: String,
    pub internal_port: u16,
    /// Qovery publishes HTTP and gRPC on 443 whatever is requested, and the provider fails the
    /// apply when the planned value differs from the one read back, so only 443 is accepted.
    #[serde(default = "default_external_port")]
    pub external_port: u16,
    #[serde(default)]
    pub protocol: BlueprintHelmPortProtocol,
    #[serde(default)]
    pub is_default: bool,
}

/// The protocols `qovery_helm.ports` accepts (terraform-provider-qovery `helm.AllowedProtocols`).
/// TCP and UDP exist on applications and containers, not on Helm services.
#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum BlueprintHelmPortProtocol {
    #[default]
    Http,
    Grpc,
}

const HELM_PUBLIC_PORT: u16 = 443;
/// Qovery puts the port name at the front of the public host (`<name>-z<env>-z<service>-gtw...`),
/// so it must be a lowercase DNS label, short enough for the whole label to stay within 63 chars.
const HELM_PORT_NAME_MAX_LENGTH: usize = 40;

fn is_valid_port_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= HELM_PORT_NAME_MAX_LENGTH
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
        && bytes[0] != b'-'
        && bytes[bytes.len() - 1] != b'-'
}

fn default_external_port() -> u16 {
    HELM_PUBLIC_PORT
}

fn validate_helm_ports(ports: &[BlueprintHelmPort]) -> Result<(), String> {
    let mut names = HashSet::new();
    for port in ports {
        // Missing fields are serde errors; these catch an explicit empty string.
        if port.name.is_empty() {
            return Err("'ports[].name' must not be empty".to_string());
        }
        if port.service_name.is_empty() {
            return Err("'ports[].serviceName' must not be empty".to_string());
        }
        if !is_valid_port_name(&port.name) {
            return Err(format!(
                "port '{}': name must be lowercase letters, digits and hyphens, not starting or ending with a hyphen, max {HELM_PORT_NAME_MAX_LENGTH} chars: it is part of the public host name",
                port.name
            ));
        }
        if !names.insert(port.name.as_str()) {
            return Err(format!("duplicate port name '{}' in 'ports'", port.name));
        }
        if port.internal_port == 0 {
            return Err(format!("port '{}': internalPort must be 1-65535", port.name));
        }
        if port.external_port != HELM_PUBLIC_PORT {
            return Err(format!(
                "port '{}': externalPort must be {HELM_PUBLIC_PORT}, the only port Qovery publishes HTTP and gRPC on",
                port.name
            ));
        }
    }
    if ports.len() > 1 && ports.iter().filter(|p| p.is_default).count() != 1 {
        return Err("exactly one entry of 'ports' must set isDefault when several ports are declared".to_string());
    }
    Ok(())
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
        let BlueprintEngine::Helm { chart, outputs, ports } = &manifest.spec.engine else {
            panic!("expected Helm engine");
        };
        assert_eq!(chart.name, "redis");
        assert_eq!(chart.repository, "https://charts.bitnami.com/bitnami");
        assert_eq!(chart.version, "20.11.3");
        assert_eq!(manifest.spec.arguments, vec!["--atomic", "--wait"]);
        assert!(manifest.spec.allow_cluster_wide_resources);
        assert_eq!(outputs.len(), 1);
        assert!(ports.is_empty(), "no ports block means no ports");
    }

    fn helm_qbm_with_ports(ports: &str) -> String {
        format!(
            r#"
kind: ServiceBlueprint
spec:
  engine:
    type: helm
    chart:
      repository: "https://grafana-community.github.io/helm-charts"
      name: "grafana"
      version: "13.2.5"
    ports:
{ports}
"#
        )
    }

    #[test]
    fn parse_helm_ports_with_defaults() {
        let yaml = helm_qbm_with_ports(
            r#"      - name: "http"
        serviceName: "grafana"
        internalPort: 80"#,
        );
        let manifest: QoveryBlueprintManifest = serde_yaml::from_str(&yaml).unwrap();
        let BlueprintEngine::Helm { ports, .. } = &manifest.spec.engine else {
            panic!("expected Helm engine");
        };
        assert_eq!(
            ports,
            &vec![BlueprintHelmPort {
                name: "http".into(),
                service_name: "grafana".into(),
                internal_port: 80,
                external_port: 443,
                protocol: BlueprintHelmPortProtocol::Http,
                is_default: false,
            }]
        );
    }

    #[test]
    fn parse_helm_ports_all_fields() {
        let yaml = helm_qbm_with_ports(
            r#"      - name: "ui"
        serviceName: "signoz"
        internalPort: 8080
        protocol: "GRPC"
        isDefault: true"#,
        );
        let manifest: QoveryBlueprintManifest = serde_yaml::from_str(&yaml).unwrap();
        let BlueprintEngine::Helm { ports, .. } = &manifest.spec.engine else {
            panic!("expected Helm engine");
        };
        assert_eq!(ports[0].external_port, 443);
        assert_eq!(ports[0].protocol, BlueprintHelmPortProtocol::Grpc);
        assert!(ports[0].is_default);
    }

    #[test]
    fn reject_invalid_helm_ports() {
        let cases = [
            (
                r#"      - name: "http"
        serviceName: "grafana"
        internalPort: 80
        protocol: "SMTP""#,
                "unknown variant `SMTP`",
            ),
            (
                r#"      - name: "db"
        serviceName: "postgres"
        internalPort: 5432
        protocol: "TCP""#,
                "unknown variant `TCP`",
            ),
            (
                r#"      - name: "http"
        serviceName: "grafana"
        internalPort: 80
        externalPort: 8443"#,
                "externalPort must be 443",
            ),
            (
                r#"      - name: "Web UI"
        serviceName: "grafana"
        internalPort: 80"#,
                "name must be lowercase letters",
            ),
            (
                r#"      - name: ""
        serviceName: "grafana"
        internalPort: 80"#,
                "'ports[].name' must not be empty",
            ),
            (
                r#"      - name: "http"
        serviceName: "grafana"
        internalPort: 0"#,
                "internalPort must be 1-65535",
            ),
            (
                r#"      - name: "http"
        serviceName: "a"
        internalPort: 80
      - name: "http"
        serviceName: "b"
        internalPort: 81"#,
                "duplicate port name",
            ),
            (
                r#"      - name: "a"
        serviceName: "a"
        internalPort: 80
      - name: "b"
        serviceName: "b"
        internalPort: 81"#,
                "exactly one entry",
            ),
        ];
        for (ports, expected) in cases {
            let err = serde_yaml::from_str::<QoveryBlueprintManifest>(&helm_qbm_with_ports(ports))
                .unwrap_err()
                .to_string();
            assert!(err.contains(expected), "expected '{expected}' in: {err}");
        }
    }

    #[test]
    fn port_name_must_be_a_short_dns_label() {
        for name in ["http", "ui-2", &"a".repeat(40)] {
            assert!(is_valid_port_name(name), "{name:?} should be accepted");
        }
        for name in ["", "Web", "web ui", "-http", "http-", "http_1", &"a".repeat(41)] {
            assert!(!is_valid_port_name(name), "{name:?} should be rejected");
        }
    }

    #[test]
    fn reject_ports_on_terraform_engine() {
        let yaml = r#"
kind: ServiceBlueprint
spec:
  engine:
    type: terraform
    provider: AWS
    terraform:
      version: "1.9.7"
    ports:
      - name: "http"
        serviceName: "x"
        internalPort: 80
"#;
        let err = serde_yaml::from_str::<QoveryBlueprintManifest>(yaml)
            .unwrap_err()
            .to_string();
        assert!(err.contains("'ports' is only supported when engine.type is 'helm'"), "{err}");
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

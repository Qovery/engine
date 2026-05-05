// Resolved blueprint spec — the final, merged configuration the engine uses for execution.
//
// Precedence (highest → lowest):
//   1. spec_overrides  (user choice, sent by q-core)
//   2. QBM spec        (blueprint author default)
//   3. Platform defaults (hardcoded here)

use crate::blueprint::models::qovery_blueprint_manifest::{
    BlueprintChart, BlueprintEngine, BlueprintOutput, BlueprintSpec, CredentialMode,
};
use crate::io_models::blueprint::BlueprintSpecOverrides;

const DEFAULT_TF_TIMEOUT_SEC: u64 = 1800;
const DEFAULT_HELM_TIMEOUT_SEC: u64 = 600;

/// Whether to use the `terraform` or `tofu` binary.
#[derive(Debug, Clone, PartialEq)]
pub enum TerraformFlavor {
    Terraform,
    OpenTofu,
}

/// Resolved spec — engine-specific, all values concrete.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedBlueprintSpec {
    Terraform(ResolvedTerraformSpec),
    Helm(ResolvedHelmSpec),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedTerraformSpec {
    pub flavor: TerraformFlavor,
    /// Cloud provider the blueprint targets (e.g. "aws", "gcp", "azure").
    pub provider: String,
    pub credential_mode: CredentialMode,
    pub timeout_sec: u64,
    pub outputs: Vec<BlueprintOutput>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedHelmSpec {
    pub chart: BlueprintChart,
    pub credential_mode: CredentialMode,
    pub timeout_sec: u64,
    pub arguments: Vec<String>,
    pub allow_cluster_wide_resources: bool,
    pub outputs: Vec<BlueprintOutput>,
}

impl ResolvedBlueprintSpec {
    /// Resolve the effective spec from the QBM defaults + optional overrides.
    pub fn resolve(spec: &BlueprintSpec, overrides: &Option<BlueprintSpecOverrides>) -> Self {
        let credential_mode = resolve_credential_mode(spec, overrides);

        match &spec.engine {
            BlueprintEngine::Terraform { provider, outputs } => {
                let timeout_sec = resolve_timeout(spec, overrides, DEFAULT_TF_TIMEOUT_SEC);
                ResolvedBlueprintSpec::Terraform(ResolvedTerraformSpec {
                    flavor: TerraformFlavor::Terraform,
                    provider: provider.clone(),
                    credential_mode,
                    timeout_sec,
                    outputs: outputs.clone(),
                })
            }
            BlueprintEngine::Opentofu { provider, outputs } => {
                let timeout_sec = resolve_timeout(spec, overrides, DEFAULT_TF_TIMEOUT_SEC);
                ResolvedBlueprintSpec::Terraform(ResolvedTerraformSpec {
                    flavor: TerraformFlavor::OpenTofu,
                    provider: provider.clone(),
                    credential_mode,
                    timeout_sec,
                    outputs: outputs.clone(),
                })
            }
            BlueprintEngine::Helm { chart, outputs } => {
                let timeout_sec = resolve_timeout(spec, overrides, DEFAULT_HELM_TIMEOUT_SEC);
                ResolvedBlueprintSpec::Helm(ResolvedHelmSpec {
                    chart: chart.clone(),
                    credential_mode,
                    timeout_sec,
                    arguments: spec.arguments.clone(),
                    allow_cluster_wide_resources: spec.allow_cluster_wide_resources,
                    outputs: outputs.clone(),
                })
            }
        }
    }
}

/// Resolve credential mode: spec_overrides.credentials > qbm.spec.credentials.default > Cluster
fn resolve_credential_mode(spec: &BlueprintSpec, overrides: &Option<BlueprintSpecOverrides>) -> CredentialMode {
    if let Some(mode) = overrides
        .as_ref()
        .and_then(|o| o.get("credentials"))
        .and_then(|v| v.as_str())
    {
        return match mode {
            "env" => CredentialMode::Env,
            _ => CredentialMode::Cluster,
        };
    }
    spec.credentials.default.clone()
}

/// Resolve timeout: spec_overrides.timeout > qbm.spec.timeout > default_timeout
fn resolve_timeout(spec: &BlueprintSpec, overrides: &Option<BlueprintSpecOverrides>, default: u64) -> u64 {
    if let Some(t) = overrides
        .as_ref()
        .and_then(|o| o.get("timeout"))
        .and_then(|v| v.as_u64())
    {
        return t;
    }
    spec.timeout.unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blueprint::models::qovery_blueprint_manifest::{
        BlueprintChart, BlueprintCredentials, BlueprintEngine, BlueprintSpec,
    };
    use std::collections::HashMap;

    fn tf_spec() -> BlueprintSpec {
        BlueprintSpec {
            engine: BlueprintEngine::Terraform {
                provider: "aws".into(),
                outputs: vec![],
            },
            credentials: BlueprintCredentials::default(),
            timeout: Some(3600),
            arguments: vec![],
            allow_cluster_wide_resources: false,
        }
    }

    fn opentofu_spec() -> BlueprintSpec {
        BlueprintSpec {
            engine: BlueprintEngine::Opentofu {
                provider: "gcp".into(),
                outputs: vec![],
            },
            credentials: BlueprintCredentials::default(),
            timeout: None,
            arguments: vec![],
            allow_cluster_wide_resources: false,
        }
    }

    fn helm_spec() -> BlueprintSpec {
        BlueprintSpec {
            engine: BlueprintEngine::Helm {
                chart: BlueprintChart {
                    repository: "https://charts.bitnami.com/bitnami".into(),
                    name: "redis".into(),
                    version: "20.11.3".into(),
                },
                outputs: vec![],
            },
            credentials: BlueprintCredentials::default(),
            timeout: None,
            arguments: vec!["--atomic".into()],
            allow_cluster_wide_resources: true,
        }
    }

    fn minimal_tf_spec() -> BlueprintSpec {
        BlueprintSpec {
            engine: BlueprintEngine::Terraform {
                provider: "aws".into(),
                outputs: vec![],
            },
            credentials: BlueprintCredentials::default(),
            timeout: None,
            arguments: vec![],
            allow_cluster_wide_resources: false,
        }
    }

    fn expect_terraform(resolved: ResolvedBlueprintSpec) -> ResolvedTerraformSpec {
        match resolved {
            ResolvedBlueprintSpec::Terraform(tf) => tf,
            _ => panic!("expected Terraform variant"),
        }
    }

    fn expect_helm(resolved: ResolvedBlueprintSpec) -> ResolvedHelmSpec {
        match resolved {
            ResolvedBlueprintSpec::Helm(helm) => helm,
            _ => panic!("expected Helm variant"),
        }
    }

    // -- Terraform --

    #[test]
    fn tf_uses_qbm_values_when_no_overrides() {
        let tf = expect_terraform(ResolvedBlueprintSpec::resolve(&tf_spec(), &None));
        assert_eq!(tf.flavor, TerraformFlavor::Terraform);
        assert_eq!(tf.provider, "aws");
        assert_eq!(tf.credential_mode, CredentialMode::Cluster);
        assert_eq!(tf.timeout_sec, 3600);
    }

    #[test]
    fn tf_uses_platform_defaults_when_qbm_omits() {
        let tf = expect_terraform(ResolvedBlueprintSpec::resolve(&minimal_tf_spec(), &None));
        assert_eq!(tf.timeout_sec, DEFAULT_TF_TIMEOUT_SEC);
    }

    #[test]
    fn opentofu_resolves_to_terraform_variant_with_opentofu_flavor() {
        let tf = expect_terraform(ResolvedBlueprintSpec::resolve(&opentofu_spec(), &None));
        assert_eq!(tf.flavor, TerraformFlavor::OpenTofu);
        assert_eq!(tf.provider, "gcp");
        assert_eq!(tf.timeout_sec, DEFAULT_TF_TIMEOUT_SEC);
    }

    // -- Helm --

    #[test]
    fn helm_uses_default_timeout() {
        let helm = expect_helm(ResolvedBlueprintSpec::resolve(&helm_spec(), &None));
        assert_eq!(helm.timeout_sec, DEFAULT_HELM_TIMEOUT_SEC);
    }

    #[test]
    fn helm_carries_arguments_and_cluster_wide() {
        let helm = expect_helm(ResolvedBlueprintSpec::resolve(&helm_spec(), &None));
        assert_eq!(helm.arguments, vec!["--atomic"]);
        assert!(helm.allow_cluster_wide_resources);
    }

    #[test]
    fn helm_carries_chart() {
        let helm = expect_helm(ResolvedBlueprintSpec::resolve(&helm_spec(), &None));
        assert_eq!(helm.chart.name, "redis");
        assert_eq!(helm.chart.version, "20.11.3");
    }

    // -- Credential overrides --

    #[test]
    fn credential_override_string_shorthand() {
        let mut overrides = HashMap::new();
        overrides.insert("credentials".into(), serde_json::json!("env"));
        let tf = expect_terraform(ResolvedBlueprintSpec::resolve(&tf_spec(), &Some(overrides)));
        assert_eq!(tf.credential_mode, CredentialMode::Env);
    }

    #[test]
    fn credential_override_unknown_value_falls_back_to_cluster() {
        let mut overrides = HashMap::new();
        overrides.insert("credentials".into(), serde_json::json!("something_else"));
        let tf = expect_terraform(ResolvedBlueprintSpec::resolve(&tf_spec(), &Some(overrides)));
        assert_eq!(tf.credential_mode, CredentialMode::Cluster);
    }

    #[test]
    fn credential_override_on_helm() {
        let mut overrides = HashMap::new();
        overrides.insert("credentials".into(), serde_json::json!("env"));
        let helm = expect_helm(ResolvedBlueprintSpec::resolve(&helm_spec(), &Some(overrides)));
        assert_eq!(helm.credential_mode, CredentialMode::Env);
    }

    // -- Timeout overrides --

    #[test]
    fn timeout_override_on_terraform() {
        let mut overrides = HashMap::new();
        overrides.insert("timeout".into(), serde_json::json!(7200));
        let tf = expect_terraform(ResolvedBlueprintSpec::resolve(&tf_spec(), &Some(overrides)));
        assert_eq!(tf.timeout_sec, 7200);
    }

    #[test]
    fn timeout_override_beats_qbm() {
        let mut overrides = HashMap::new();
        overrides.insert("timeout".into(), serde_json::json!(300));
        let tf = expect_terraform(ResolvedBlueprintSpec::resolve(&tf_spec(), &Some(overrides)));
        assert_eq!(tf.timeout_sec, 300);
    }

    #[test]
    fn timeout_override_on_helm() {
        let mut overrides = HashMap::new();
        overrides.insert("timeout".into(), serde_json::json!(120));
        let helm = expect_helm(ResolvedBlueprintSpec::resolve(&helm_spec(), &Some(overrides)));
        assert_eq!(helm.timeout_sec, 120);
    }

    // -- Multiple overrides --

    #[test]
    fn multiple_overrides_on_terraform() {
        let mut overrides = HashMap::new();
        overrides.insert("credentials".into(), serde_json::json!("env"));
        overrides.insert("timeout".into(), serde_json::json!(900));
        let tf = expect_terraform(ResolvedBlueprintSpec::resolve(&tf_spec(), &Some(overrides)));
        assert_eq!(tf.credential_mode, CredentialMode::Env);
        assert_eq!(tf.timeout_sec, 900);
    }

    // -- Empty overrides --

    #[test]
    fn empty_overrides_map_same_as_none() {
        let empty: Option<BlueprintSpecOverrides> = Some(HashMap::new());
        let resolved_empty = ResolvedBlueprintSpec::resolve(&tf_spec(), &empty);
        let resolved_none = ResolvedBlueprintSpec::resolve(&tf_spec(), &None);
        assert_eq!(resolved_empty, resolved_none);
    }
}

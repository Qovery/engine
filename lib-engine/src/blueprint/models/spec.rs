// Resolved blueprint spec — the final, merged configuration the engine uses for execution.
//
// Precedence (highest → lowest):
//   1. spec_overrides  (user choice, sent by q-core)
//   2. QBM spec        (blueprint author default)
//   3. Platform defaults (hardcoded here)

use crate::blueprint::models::error::BlueprintError;
use crate::blueprint::models::qovery_blueprint_manifest::{
    BackendMode, BlueprintChart, BlueprintEngine, BlueprintOutput, BlueprintResources, BlueprintSpec, CredentialMode,
    QoveryBlueprintManifest,
};
use crate::io_models::blueprint::BlueprintSpecOverrides;
use serde::Serialize;
use std::collections::HashMap;

const DEFAULT_TF_TIMEOUT_SEC: u64 = 1800;
const DEFAULT_HELM_TIMEOUT_SEC: u64 = 600;

const DEFAULT_JOB_CPU_MILLI: u32 = 500;
const DEFAULT_JOB_RAM_MIB: u32 = 512;
const DEFAULT_JOB_STORAGE_GIB: u32 = 20;

pub const DEFAULT_BLUEPRINT_DESCRIPTION: &str = "Deployed from blueprint";

#[derive(Debug, Clone, PartialEq)]
pub struct JobResources {
    pub cpu_milli: u32,
    pub ram_mib: u32,
    pub storage_gib: u32,
}

/// Variable representation for Tera template rendering.
#[derive(Serialize)]
pub struct TemplateVariable {
    pub name: String,
    pub value: String,
    pub is_secret: bool,
}

/// Whether to use the `terraform` or `tofu` binary.
#[derive(Debug, Clone, PartialEq)]
pub enum TerraformFlavor {
    Terraform,
    OpenTofu,
}

/// Resolved backend configuration for the created Terraform service.
/// Maps to the `backend` attribute on `qovery_terraform_service`.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedBackend {
    /// Qovery-managed Kubernetes backend. The created service uses `backend { kubernetes {} }`.
    /// State is managed by Qovery in a K8s Secret.
    Qovery,
    /// Blueprint-managed backend. The user provides backend type + config at creation time.
    /// The Qovery Terraform provider passes this to the platform, which generates
    /// backend.tf during the service's Docker image build.
    Blueprint {
        backend_type: String,
        config: HashMap<String, String>,
    },
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
    pub description: String,
    pub credential_mode: CredentialMode,
    pub backend: ResolvedBackend,
    pub timeout_sec: u64,
    pub outputs: Vec<BlueprintOutput>,
    pub job_resources: JobResources,
    pub engine_version: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedHelmSpec {
    pub chart: BlueprintChart,
    pub description: String,
    pub credential_mode: CredentialMode,
    pub timeout_sec: u64,
    pub arguments: Vec<String>,
    pub allow_cluster_wide_resources: bool,
    pub outputs: Vec<BlueprintOutput>,
}

impl ResolvedBlueprintSpec {
    /// Resolve the effective spec from the QBM defaults + optional overrides.
    pub fn resolve(
        manifest: &QoveryBlueprintManifest,
        overrides: &Option<BlueprintSpecOverrides>,
    ) -> Result<Self, BlueprintError> {
        let spec = &manifest.spec;
        let description = manifest
            .metadata
            .description
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_BLUEPRINT_DESCRIPTION)
            .to_string();
        let credential_mode = resolve_credential_mode(spec, overrides);

        let resolved = match &spec.engine {
            BlueprintEngine::Terraform { provider, outputs } => {
                let timeout_sec = resolve_timeout(spec, overrides, DEFAULT_TF_TIMEOUT_SEC);
                let backend = resolve_backend(spec, overrides);
                let job_resources = resolve_job_resources(spec.resources.as_ref(), overrides);
                ResolvedBlueprintSpec::Terraform(ResolvedTerraformSpec {
                    flavor: TerraformFlavor::Terraform,
                    provider: provider.clone(),
                    description,
                    credential_mode,
                    backend,
                    timeout_sec,
                    outputs: outputs.clone(),
                    job_resources,
                    engine_version: resolve_engine_version(spec, overrides)
                        .ok_or(BlueprintError::MissingEngineVersion)?,
                })
            }
            BlueprintEngine::Opentofu { provider, outputs } => {
                let timeout_sec = resolve_timeout(spec, overrides, DEFAULT_TF_TIMEOUT_SEC);
                let backend = resolve_backend(spec, overrides);
                let job_resources = resolve_job_resources(spec.resources.as_ref(), overrides);
                ResolvedBlueprintSpec::Terraform(ResolvedTerraformSpec {
                    flavor: TerraformFlavor::OpenTofu,
                    provider: provider.clone(),
                    description,
                    credential_mode,
                    backend,
                    timeout_sec,
                    outputs: outputs.clone(),
                    job_resources,
                    engine_version: resolve_engine_version(spec, overrides)
                        .ok_or(BlueprintError::MissingEngineVersion)?,
                })
            }
            BlueprintEngine::Helm { chart, outputs } => {
                let timeout_sec = resolve_timeout(spec, overrides, DEFAULT_HELM_TIMEOUT_SEC);
                ResolvedBlueprintSpec::Helm(ResolvedHelmSpec {
                    chart: chart.clone(),
                    description,
                    credential_mode,
                    timeout_sec,
                    arguments: spec.arguments.clone(),
                    allow_cluster_wide_resources: spec.allow_cluster_wide_resources,
                    outputs: outputs.clone(),
                })
            }
        };
        Ok(resolved)
    }
}

/// Resolve engine_version: spec_overrides.engine_version > qbm.spec.engine_version.
fn resolve_engine_version(spec: &BlueprintSpec, overrides: &Option<BlueprintSpecOverrides>) -> Option<String> {
    overrides
        .as_ref()
        .and_then(|o| o.get("engine_version"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| spec.engine_version.clone())
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

/// Resolve backend: spec_overrides.backend > qbm.spec.backend.default > Qovery.
/// When mode is Blueprint, the backend config comes from the QBM spec (not from overrides).
fn resolve_backend(spec: &BlueprintSpec, overrides: &Option<BlueprintSpecOverrides>) -> ResolvedBackend {
    let mode = if let Some(mode_str) = overrides
        .as_ref()
        .and_then(|o| o.get("backend"))
        .and_then(|v| v.as_str())
    {
        match mode_str {
            "user_provided" => BackendMode::Blueprint,
            _ => BackendMode::Qovery,
        }
    } else {
        spec.backend.default.clone()
    };

    match mode {
        BackendMode::Qovery => ResolvedBackend::Qovery,
        BackendMode::Blueprint => {
            let (backend_type, config) = spec
                .backend
                .user_provided
                .as_ref()
                .map(|c| (c.backend_type.clone(), c.config.clone()))
                .unwrap_or_else(|| ("local".to_string(), HashMap::new()));
            ResolvedBackend::Blueprint { backend_type, config }
        }
    }
}

/// Parse a Kubernetes-style CPU string (e.g. "500m", "1000m", "2") to millicores.
fn parse_milli(s: &str) -> Option<u32> {
    let s = s.trim();
    if let Some(stripped) = s.strip_suffix('m') {
        stripped.parse::<u32>().ok()
    } else {
        // Whole cores (e.g. "2" → 2000m)
        s.parse::<u32>().ok().map(|v| v * 1000)
    }
}

/// Parse a Kubernetes-style memory string (e.g. "512Mi", "1Gi", "256") to MiB.
fn parse_mib(s: &str) -> Option<u32> {
    let s = s.trim();
    if let Some(stripped) = s.strip_suffix("Gi") {
        stripped.parse::<u32>().ok().map(|v| v * 1024)
    } else if let Some(stripped) = s.strip_suffix("Mi") {
        stripped.parse::<u32>().ok()
    } else {
        // Bare number treated as MiB
        s.parse::<u32>().ok()
    }
}

/// Parse a storage string (e.g. "20Gi", "10") to GiB.
fn parse_gib(s: &str) -> Option<u32> {
    let s = s.trim();
    if let Some(stripped) = s.strip_suffix("Gi") {
        stripped.parse::<u32>().ok()
    } else {
        // Bare number treated as GiB
        s.parse::<u32>().ok()
    }
}

/// Resolve job resources: spec_overrides.resources > qbm.spec.resources > platform defaults.
fn resolve_job_resources(
    qbm_resources: Option<&BlueprintResources>,
    overrides: &Option<BlueprintSpecOverrides>,
) -> JobResources {
    let override_resources = overrides.as_ref().and_then(|o| o.get("resources"));

    let cpu_milli = override_resources
        .and_then(|r| r.get("cpu"))
        .and_then(|v| v.as_str())
        .and_then(parse_milli)
        .or_else(|| qbm_resources.and_then(|r| r.cpu.as_deref()).and_then(parse_milli))
        .unwrap_or(DEFAULT_JOB_CPU_MILLI);

    let ram_mib = override_resources
        .and_then(|r| r.get("ram"))
        .and_then(|v| v.as_str())
        .and_then(parse_mib)
        .or_else(|| qbm_resources.and_then(|r| r.ram.as_deref()).and_then(parse_mib))
        .unwrap_or(DEFAULT_JOB_RAM_MIB);

    let storage_gib = override_resources
        .and_then(|r| r.get("storage"))
        .and_then(|v| v.as_str())
        .and_then(parse_gib)
        .or_else(|| qbm_resources.and_then(|r| r.storage.as_deref()).and_then(parse_gib))
        .unwrap_or(DEFAULT_JOB_STORAGE_GIB);

    JobResources {
        cpu_milli,
        ram_mib,
        storage_gib,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blueprint::models::error::BlueprintError;
    use crate::blueprint::models::qovery_blueprint_manifest::{
        BlueprintBackend, BlueprintChart, BlueprintCredentials, BlueprintEngine, BlueprintKind, BlueprintMetadata,
        BlueprintSpec,
    };
    use std::collections::HashMap;

    fn manifest(spec: BlueprintSpec) -> QoveryBlueprintManifest {
        QoveryBlueprintManifest {
            kind: BlueprintKind::ServiceBlueprint,
            metadata: BlueprintMetadata::default(),
            spec,
        }
    }

    fn tf_spec() -> BlueprintSpec {
        BlueprintSpec {
            engine: BlueprintEngine::Terraform {
                provider: "aws".into(),
                outputs: vec![],
            },
            credentials: BlueprintCredentials::default(),
            backend: BlueprintBackend::default(),
            timeout: Some(3600),
            arguments: vec![],
            allow_cluster_wide_resources: false,
            resources: None,
            engine_version: Some("1.9.7".into()),
        }
    }

    fn opentofu_spec() -> BlueprintSpec {
        BlueprintSpec {
            engine: BlueprintEngine::Opentofu {
                provider: "gcp".into(),
                outputs: vec![],
            },
            credentials: BlueprintCredentials::default(),
            backend: BlueprintBackend::default(),
            timeout: None,
            arguments: vec![],
            allow_cluster_wide_resources: false,
            resources: None,
            engine_version: Some("1.9.7".into()),
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
            backend: BlueprintBackend::default(),
            timeout: None,
            arguments: vec!["--atomic".into()],
            allow_cluster_wide_resources: true,
            resources: None,
            engine_version: None,
        }
    }

    fn minimal_tf_spec() -> BlueprintSpec {
        BlueprintSpec {
            engine: BlueprintEngine::Terraform {
                provider: "aws".into(),
                outputs: vec![],
            },
            credentials: BlueprintCredentials::default(),
            backend: BlueprintBackend::default(),
            timeout: None,
            arguments: vec![],
            allow_cluster_wide_resources: false,
            resources: None,
            engine_version: Some("1.9.7".into()),
        }
    }

    fn expect_terraform(resolved: Result<ResolvedBlueprintSpec, BlueprintError>) -> ResolvedTerraformSpec {
        match resolved.unwrap() {
            ResolvedBlueprintSpec::Terraform(tf) => tf,
            _ => panic!("expected Terraform variant"),
        }
    }

    fn expect_helm(resolved: Result<ResolvedBlueprintSpec, BlueprintError>) -> ResolvedHelmSpec {
        match resolved.unwrap() {
            ResolvedBlueprintSpec::Helm(helm) => helm,
            _ => panic!("expected Helm variant"),
        }
    }

    // -- Terraform --

    #[test]
    fn engine_version_override_wins_over_qbm() {
        let mut overrides = HashMap::new();
        overrides.insert("engine_version".into(), serde_json::json!("1.5.7"));
        let tf = expect_terraform(ResolvedBlueprintSpec::resolve(&manifest(tf_spec()), &Some(overrides)));
        assert_eq!(tf.engine_version, "1.5.7");
    }

    #[test]
    fn engine_version_falls_back_to_qbm_when_override_empty_or_absent() {
        let mut overrides = HashMap::new();
        overrides.insert("engine_version".into(), serde_json::json!(""));
        let tf = expect_terraform(ResolvedBlueprintSpec::resolve(&manifest(tf_spec()), &Some(overrides)));
        assert_eq!(tf.engine_version, "1.9.7");
    }

    #[test]
    fn tf_uses_qbm_values_when_no_overrides() {
        let tf = expect_terraform(ResolvedBlueprintSpec::resolve(&manifest(tf_spec()), &None));
        assert_eq!(tf.flavor, TerraformFlavor::Terraform);
        assert_eq!(tf.provider, "aws");
        assert_eq!(tf.credential_mode, CredentialMode::Cluster);
        assert_eq!(tf.timeout_sec, 3600);
    }

    #[test]
    fn tf_uses_platform_defaults_when_qbm_omits() {
        let tf = expect_terraform(ResolvedBlueprintSpec::resolve(&manifest(minimal_tf_spec()), &None));
        assert_eq!(tf.timeout_sec, DEFAULT_TF_TIMEOUT_SEC);
    }

    #[test]
    fn opentofu_resolves_to_terraform_variant_with_opentofu_flavor() {
        let tf = expect_terraform(ResolvedBlueprintSpec::resolve(&manifest(opentofu_spec()), &None));
        assert_eq!(tf.flavor, TerraformFlavor::OpenTofu);
        assert_eq!(tf.provider, "gcp");
        assert_eq!(tf.timeout_sec, DEFAULT_TF_TIMEOUT_SEC);
    }

    // -- Helm --

    #[test]
    fn helm_uses_default_timeout() {
        let helm = expect_helm(ResolvedBlueprintSpec::resolve(&manifest(helm_spec()), &None));
        assert_eq!(helm.timeout_sec, DEFAULT_HELM_TIMEOUT_SEC);
    }

    #[test]
    fn helm_carries_arguments_and_cluster_wide() {
        let helm = expect_helm(ResolvedBlueprintSpec::resolve(&manifest(helm_spec()), &None));
        assert_eq!(helm.arguments, vec!["--atomic"]);
        assert!(helm.allow_cluster_wide_resources);
    }

    #[test]
    fn helm_carries_chart() {
        let helm = expect_helm(ResolvedBlueprintSpec::resolve(&manifest(helm_spec()), &None));
        assert_eq!(helm.chart.name, "redis");
        assert_eq!(helm.chart.version, "20.11.3");
    }

    // -- Credential overrides --

    #[test]
    fn credential_override_string_shorthand() {
        let mut overrides = HashMap::new();
        overrides.insert("credentials".into(), serde_json::json!("env"));
        let tf = expect_terraform(ResolvedBlueprintSpec::resolve(&manifest(tf_spec()), &Some(overrides)));
        assert_eq!(tf.credential_mode, CredentialMode::Env);
    }

    #[test]
    fn credential_override_unknown_value_falls_back_to_cluster() {
        let mut overrides = HashMap::new();
        overrides.insert("credentials".into(), serde_json::json!("something_else"));
        let tf = expect_terraform(ResolvedBlueprintSpec::resolve(&manifest(tf_spec()), &Some(overrides)));
        assert_eq!(tf.credential_mode, CredentialMode::Cluster);
    }

    #[test]
    fn credential_override_on_helm() {
        let mut overrides = HashMap::new();
        overrides.insert("credentials".into(), serde_json::json!("env"));
        let helm = expect_helm(ResolvedBlueprintSpec::resolve(&manifest(helm_spec()), &Some(overrides)));
        assert_eq!(helm.credential_mode, CredentialMode::Env);
    }

    // -- Timeout overrides --

    #[test]
    fn timeout_override_on_terraform() {
        let mut overrides = HashMap::new();
        overrides.insert("timeout".into(), serde_json::json!(7200));
        let tf = expect_terraform(ResolvedBlueprintSpec::resolve(&manifest(tf_spec()), &Some(overrides)));
        assert_eq!(tf.timeout_sec, 7200);
    }

    #[test]
    fn timeout_override_beats_qbm() {
        let mut overrides = HashMap::new();
        overrides.insert("timeout".into(), serde_json::json!(300));
        let tf = expect_terraform(ResolvedBlueprintSpec::resolve(&manifest(tf_spec()), &Some(overrides)));
        assert_eq!(tf.timeout_sec, 300);
    }

    #[test]
    fn timeout_override_on_helm() {
        let mut overrides = HashMap::new();
        overrides.insert("timeout".into(), serde_json::json!(120));
        let helm = expect_helm(ResolvedBlueprintSpec::resolve(&manifest(helm_spec()), &Some(overrides)));
        assert_eq!(helm.timeout_sec, 120);
    }

    // -- Multiple overrides --

    #[test]
    fn multiple_overrides_on_terraform() {
        let mut overrides = HashMap::new();
        overrides.insert("credentials".into(), serde_json::json!("env"));
        overrides.insert("timeout".into(), serde_json::json!(900));
        let tf = expect_terraform(ResolvedBlueprintSpec::resolve(&manifest(tf_spec()), &Some(overrides)));
        assert_eq!(tf.credential_mode, CredentialMode::Env);
        assert_eq!(tf.timeout_sec, 900);
    }

    // -- Backend overrides --

    #[test]
    fn backend_defaults_to_qovery() {
        let tf = expect_terraform(ResolvedBlueprintSpec::resolve(&manifest(tf_spec()), &None));
        assert_eq!(tf.backend, ResolvedBackend::Qovery);
    }

    #[test]
    fn backend_override_to_blueprint_uses_qbm_config() {
        // QBM has no blueprint backend config (default spec) → falls back to local backend
        let mut overrides = HashMap::new();
        overrides.insert("backend".into(), serde_json::json!("user_provided"));
        let tf = expect_terraform(ResolvedBlueprintSpec::resolve(&manifest(tf_spec()), &Some(overrides)));
        assert_eq!(
            tf.backend,
            ResolvedBackend::Blueprint {
                backend_type: "local".to_string(),
                config: HashMap::new(),
            }
        );
    }

    #[test]
    fn backend_override_to_blueprint_with_qbm_config() {
        use crate::blueprint::models::qovery_blueprint_manifest::BlueprintBackendConfig;

        let mut spec = tf_spec();
        spec.backend.user_provided = Some(BlueprintBackendConfig {
            backend_type: "s3".to_string(),
            config: HashMap::from([
                ("bucket".to_string(), "my-state-bucket".to_string()),
                ("region".to_string(), "eu-west-3".to_string()),
            ]),
        });

        let mut overrides = HashMap::new();
        overrides.insert("backend".into(), serde_json::json!("user_provided"));
        let tf = expect_terraform(ResolvedBlueprintSpec::resolve(&manifest(spec), &Some(overrides)));
        match &tf.backend {
            ResolvedBackend::Blueprint { backend_type, config } => {
                assert_eq!(backend_type, "s3");
                assert_eq!(config["bucket"], "my-state-bucket");
                assert_eq!(config["region"], "eu-west-3");
            }
            _ => panic!("expected Blueprint backend"),
        }
    }

    #[test]
    fn backend_override_unknown_value_falls_back_to_qovery() {
        let mut overrides = HashMap::new();
        overrides.insert("backend".into(), serde_json::json!("something_else"));
        let tf = expect_terraform(ResolvedBlueprintSpec::resolve(&manifest(tf_spec()), &Some(overrides)));
        assert_eq!(tf.backend, ResolvedBackend::Qovery);
    }

    // -- Empty overrides --

    #[test]
    fn empty_overrides_map_same_as_none() {
        let empty: Option<BlueprintSpecOverrides> = Some(HashMap::new());
        let resolved_empty = ResolvedBlueprintSpec::resolve(&manifest(tf_spec()), &empty);
        let resolved_none = ResolvedBlueprintSpec::resolve(&manifest(tf_spec()), &None);
        assert_eq!(resolved_empty, resolved_none);
    }

    // -- Metadata description --

    #[test]
    fn description_propagates_from_qbm_metadata_to_terraform() {
        let mut m = manifest(tf_spec());
        m.metadata.description = Some("S3 bucket with encryption".into());
        let tf = expect_terraform(ResolvedBlueprintSpec::resolve(&m, &None));
        assert_eq!(tf.description, "S3 bucket with encryption");
    }

    #[test]
    fn description_falls_back_to_default_when_absent() {
        let tf = expect_terraform(ResolvedBlueprintSpec::resolve(&manifest(tf_spec()), &None));
        assert_eq!(tf.description, DEFAULT_BLUEPRINT_DESCRIPTION);
    }

    #[test]
    fn description_falls_back_to_default_when_qbm_empty_string() {
        let mut m = manifest(tf_spec());
        m.metadata.description = Some(String::new());
        let tf = expect_terraform(ResolvedBlueprintSpec::resolve(&m, &None));
        assert_eq!(tf.description, DEFAULT_BLUEPRINT_DESCRIPTION);
    }

    #[test]
    fn description_propagates_to_helm() {
        let mut m = manifest(helm_spec());
        m.metadata.description = Some("Redis cache".into());
        let helm = expect_helm(ResolvedBlueprintSpec::resolve(&m, &None));
        assert_eq!(helm.description, "Redis cache");
    }

    #[test]
    fn description_falls_back_to_default_on_helm_when_absent() {
        let helm = expect_helm(ResolvedBlueprintSpec::resolve(&manifest(helm_spec()), &None));
        assert_eq!(helm.description, DEFAULT_BLUEPRINT_DESCRIPTION);
    }

    // -- Job resources --

    #[test]
    fn job_resources_defaults_when_no_qbm_resources() {
        let tf = expect_terraform(ResolvedBlueprintSpec::resolve(&manifest(tf_spec()), &None));
        assert_eq!(tf.job_resources.cpu_milli, 500);
        assert_eq!(tf.job_resources.ram_mib, 512);
        assert_eq!(tf.job_resources.storage_gib, 20);
    }

    #[test]
    fn job_resources_from_qbm() {
        use crate::blueprint::models::qovery_blueprint_manifest::BlueprintResources;
        let mut spec = tf_spec();
        spec.resources = Some(BlueprintResources {
            cpu: Some("1000m".into()),
            ram: Some("2Gi".into()),
            storage: Some("50Gi".into()),
        });
        let tf = expect_terraform(ResolvedBlueprintSpec::resolve(&manifest(spec), &None));
        assert_eq!(tf.job_resources.cpu_milli, 1000);
        assert_eq!(tf.job_resources.ram_mib, 2048);
        assert_eq!(tf.job_resources.storage_gib, 50);
    }

    #[test]
    fn job_resources_overrides_beat_qbm() {
        use crate::blueprint::models::qovery_blueprint_manifest::BlueprintResources;
        let mut spec = tf_spec();
        spec.resources = Some(BlueprintResources {
            cpu: Some("500m".into()),
            ram: Some("512Mi".into()),
            storage: Some("10Gi".into()),
        });
        let mut overrides = HashMap::new();
        overrides.insert(
            "resources".into(),
            serde_json::json!({ "cpu": "2000m", "ram": "4Gi", "storage": "100Gi" }),
        );
        let tf = expect_terraform(ResolvedBlueprintSpec::resolve(&manifest(spec), &Some(overrides)));
        assert_eq!(tf.job_resources.cpu_milli, 2000);
        assert_eq!(tf.job_resources.ram_mib, 4096);
        assert_eq!(tf.job_resources.storage_gib, 100);
    }

    #[test]
    fn job_resources_partial_override() {
        let mut overrides = HashMap::new();
        overrides.insert("resources".into(), serde_json::json!({ "cpu": "750m" }));
        let tf = expect_terraform(ResolvedBlueprintSpec::resolve(&manifest(tf_spec()), &Some(overrides)));
        assert_eq!(tf.job_resources.cpu_milli, 750);
        // ram and storage fall back to defaults
        assert_eq!(tf.job_resources.ram_mib, 512);
        assert_eq!(tf.job_resources.storage_gib, 20);
    }

    // -- Engine version --

    #[test]
    fn engine_version_propagates_from_qbm() {
        let mut spec = tf_spec();
        spec.engine_version = Some("1.5.7".into());
        let tf = expect_terraform(ResolvedBlueprintSpec::resolve(&manifest(spec), &None));
        assert_eq!(tf.engine_version, "1.5.7");
    }

    #[test]
    fn engine_version_propagates_for_opentofu() {
        let mut spec = opentofu_spec();
        spec.engine_version = Some("1.10.3".into());
        let tf = expect_terraform(ResolvedBlueprintSpec::resolve(&manifest(spec), &None));
        assert_eq!(tf.engine_version, "1.10.3");
    }

    #[test]
    fn missing_engine_version_on_terraform_returns_error() {
        let mut spec = tf_spec();
        spec.engine_version = None;
        assert_eq!(
            ResolvedBlueprintSpec::resolve(&manifest(spec), &None),
            Err(BlueprintError::MissingEngineVersion)
        );
    }

    #[test]
    fn missing_engine_version_on_opentofu_returns_error() {
        let mut spec = opentofu_spec();
        spec.engine_version = None;
        assert_eq!(
            ResolvedBlueprintSpec::resolve(&manifest(spec), &None),
            Err(BlueprintError::MissingEngineVersion)
        );
    }
}

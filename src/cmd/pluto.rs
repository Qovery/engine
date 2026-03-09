use crate::cmd::command::{CommandError as RawCommandError, CommandKiller, ExecutableCommand, QoveryCommand};
use crate::cmd::kubent::Deprecation;
use crate::errors::CommandError;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

#[derive(thiserror::Error, Clone, Debug, PartialEq)]
pub enum PlutoError {
    #[error("Kubernetes config file path is not valid or does not exist: {kubeconfig_path}")]
    InvalidKubeConfig { kubeconfig_path: String },
    #[error("Pluto command terminated with an error: {error:?}")]
    CmdError { error: CommandError },
    #[error("Pluto command generated an invalid output: {output}")]
    InvalidCmdOutputError { output: String },
}

#[derive(Clone)]
struct PlutoCmdOutput {
    stdout: Option<String>,
}

struct PlutoCmd {}

impl PlutoCmd {
    pub fn new() -> Self {
        Self {}
    }

    pub fn get_deprecations(
        &self,
        kubeconfig: &Path,
        target_version: Option<String>,
        envs: &[(&str, &str)],
    ) -> Result<PlutoCmdOutput, CommandError> {
        let mut args = vec![
            "detect-all-in-cluster".to_string(),
            "--kubeconfig".to_string(),
            kubeconfig.to_str().unwrap_or_default().to_string(),
            "--output".to_string(),
            "json".to_string(),
        ];

        if let Some(target_version) = target_version {
            args.push("--target-versions".to_string());
            args.push(format!("k8s=v{}", target_version.trim_start_matches('v')));
        }

        let args_ref = args.iter().map(String::as_str).collect::<Vec<_>>();
        let mut envs_with_soft_memory_limit = envs.to_vec();
        if !envs.iter().any(|(k, _v)| k == &"GOMEMLIMIT") {
            // Set a soft memory limit of 64MiB for pluto since it can eventually OOM.
            // This is not a hard limit, it's just a hint to the Go runtime trying to keep
            // memory under the limit by triggering GC more often.
            envs_with_soft_memory_limit.push(("GOMEMLIMIT", "64MiB"));
        }

        let mut cmd = QoveryCommand::new("pluto", args_ref.as_slice(), envs_with_soft_memory_limit.as_slice());
        let mut stdout_output: Vec<String> = Vec::new();

        let stdout_output_formatter = &mut |line| {
            stdout_output.push(line);
        };

        match cmd.exec_with_abort(
            stdout_output_formatter,
            &mut |line| warn!("pluto stderr: {}", line),
            &CommandKiller::from_timeout(Duration::from_secs(10 * 60)),
        ) {
            Ok(_) => Ok(PlutoCmdOutput {
                stdout: if stdout_output.is_empty() {
                    None
                } else {
                    Some(stdout_output.join(""))
                },
            }),
            Err(err) if is_expected_pluto_result_exit_status(&err) => Ok(PlutoCmdOutput {
                stdout: if stdout_output.is_empty() {
                    None
                } else {
                    Some(stdout_output.join(""))
                },
            }),
            Err(err) => Err(CommandError::new(
                "Cannot get deprecations".to_string(),
                Some(format!("command failed: {err:?}")),
                None,
            )),
        }
    }
}

fn is_expected_pluto_result_exit_status(error: &RawCommandError) -> bool {
    match error {
        RawCommandError::ExitStatusError(exit_status) => {
            matches!(exit_status.code(), Some(2..=4))
        }
        _ => false,
    }
}

pub struct Pluto {
    pluto_cmd: PlutoCmd,
}

impl Default for Pluto {
    fn default() -> Self {
        Self::new()
    }
}

impl Pluto {
    pub fn new() -> Self {
        Self {
            pluto_cmd: PlutoCmd::new(),
        }
    }

    pub fn get_deprecations(
        &self,
        kubeconfig: &Path,
        target_version: Option<String>,
        envs: &[(&str, &str)],
    ) -> Result<Vec<Deprecation>, PlutoError> {
        if !kubeconfig.exists() {
            return Err(PlutoError::InvalidKubeConfig {
                kubeconfig_path: kubeconfig.display().to_string(),
            });
        }

        match self.pluto_cmd.get_deprecations(kubeconfig, target_version, envs) {
            Ok(out) => {
                let Some(stdout) = out.stdout else {
                    return Ok(Vec::new());
                };
                if stdout.trim().is_empty() {
                    return Ok(Vec::new());
                }
                parse_deprecations(&stdout)
            }
            Err(err) => Err(PlutoError::CmdError { error: err }),
        }
    }
}

fn parse_deprecations(stdout: &str) -> Result<Vec<Deprecation>, PlutoError> {
    let output: PlutoJsonOutput = serde_json::from_str(stdout).map_err(|e| PlutoError::InvalidCmdOutputError {
        output: format!("Cannot parse strict Pluto JSON output: {e}"),
    })?;

    Ok(output
        .items
        .into_iter()
        .map(|entry| {
            let deprecated_in = entry.api.deprecated_in.as_deref();
            let removed_in = entry.api.removed_in.as_deref();
            let since = deprecated_in
                .or(removed_in)
                .map(normalize_semver)
                .filter(|v| !v.is_empty());

            Deprecation {
                name: normalize_output_value(entry.name),
                namespace: normalize_output_value(entry.namespace),
                kind: Some(entry.api.kind),
                api_version: Some(entry.api.version),
                rule_set: entry
                    .api
                    .component
                    .or_else(|| since.as_ref().map(|v| format!("Deprecated APIs removed in {v}"))),
                replace_with: normalize_output_value(entry.api.replacement_api),
                since,
            }
        })
        .collect())
}

fn normalize_semver(version: &str) -> String {
    version.trim().trim_start_matches('v').to_string()
}

fn normalize_output_value(input: Option<String>) -> Option<String> {
    input.and_then(|value| {
        let normalized = value.trim();
        if normalized.is_empty()
            || normalized.eq_ignore_ascii_case("<undefined>")
            || normalized.eq_ignore_ascii_case("<unknown>")
        {
            None
        } else {
            Some(normalized.to_string())
        }
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlutoJsonOutput {
    #[serde(default)]
    items: Vec<PlutoJsonItem>,
    #[serde(rename = "target-versions")]
    _target_versions: Option<HashMap<String, String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlutoJsonItem {
    name: Option<String>,
    namespace: Option<String>,
    api: PlutoJsonApi,
    #[serde(rename = "deprecated")]
    _deprecated: bool,
    #[serde(rename = "removed")]
    _removed: bool,
    #[serde(rename = "filePath")]
    _file_path: Option<String>,
    #[serde(rename = "replacementAvailable")]
    _replacement_available: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlutoJsonApi {
    version: String,
    kind: String,
    #[serde(rename = "deprecated-in")]
    deprecated_in: Option<String>,
    #[serde(rename = "removed-in")]
    removed_in: Option<String>,
    #[serde(rename = "replacement-api")]
    replacement_api: Option<String>,
    #[serde(rename = "replacement-available-in")]
    _replacement_available_in: Option<String>,
    component: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{is_expected_pluto_result_exit_status, parse_deprecations};
    use crate::cmd::command::CommandError as RawCommandError;
    use std::os::unix::process::ExitStatusExt;

    #[test]
    fn test_parse_deprecations_with_valid_strict_json() {
        let payload = r#"
{
  "items": [
    {
      "name": "ingress-nginx",
      "namespace": "default",
      "api": {
        "version": "networking.k8s.io/v1beta1",
        "kind": "Ingress",
        "deprecated-in": "v1.19.0",
        "removed-in": "v1.22.0",
        "replacement-api": "networking.k8s.io/v1",
        "component": "k8s"
      },
      "deprecated": true,
      "removed": false
    }
  ],
  "target-versions": {
    "k8s": "v1.33.0"
  }
}
"#;

        let result = parse_deprecations(payload).expect("Parse should succeed");
        assert_eq!(1, result.len());
        assert_eq!(Some("ingress-nginx".to_string()), result[0].name);
        assert_eq!(Some("1.19.0".to_string()), result[0].since);
    }

    #[test]
    fn test_parse_deprecations_empty_items() {
        let payload = r#"
{
  "items": [],
  "target-versions": {
    "k8s": "v1.33.0"
  }
}
"#;

        let result = parse_deprecations(payload).expect("Parse should succeed");
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_deprecations_invalid_json() {
        let payload = "not-json";
        let result = parse_deprecations(payload);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_deprecations_with_real_pluto_detect_files_output() {
        let payload = r#"
{
  "items": [
    {
      "name": "ingress-nginx",
      "filePath": "/tmp/manifests/legacy-ingress.yaml",
      "namespace": "default",
      "api": {
        "version": "networking.k8s.io/v1beta1",
        "kind": "Ingress",
        "replacement-api": "networking.k8s.io/v1",
        "deprecated-in": "v1.19.0",
        "removed-in": "v1.22.0",
        "component": "k8s"
      },
      "deprecated": true,
      "removed": true,
      "replacementAvailable": true
    }
  ],
  "target-versions": {
    "k8s": "v1.33.0"
  }
}
"#;

        let result = parse_deprecations(payload).expect("Parse should succeed");
        assert_eq!(1, result.len());
        assert_eq!(Some("ingress-nginx".to_string()), result[0].name);
        assert_eq!(Some("Ingress".to_string()), result[0].kind);
        assert_eq!(Some("networking.k8s.io/v1beta1".to_string()), result[0].api_version);
        assert_eq!(Some("networking.k8s.io/v1".to_string()), result[0].replace_with);
        assert_eq!(Some("1.19.0".to_string()), result[0].since);
        assert_eq!(Some("k8s".to_string()), result[0].rule_set);
    }

    #[test]
    fn test_parse_deprecations_with_pluto_docs_detect_helm_output() {
        let payload = r#"
{
  "items": [
    {
      "name": "cert-manager/cert-manager-webhook",
      "namespace": "cert-manager",
      "api": {
        "version": "admissionregistration.k8s.io/v1beta1",
        "kind": "MutatingWebhookConfiguration",
        "deprecated-in": "v1.16.0",
        "removed-in": "v1.19.0",
        "replacement-api": "admissionregistration.k8s.io/v1",
        "component": "k8s"
      },
      "deprecated": true,
      "removed": false
    }
  ],
  "target-versions": {
    "cert-manager": "v0.15.1",
    "istio": "v1.6.0",
    "k8s": "v1.16.0"
  }
}
"#;

        let result = parse_deprecations(payload).expect("Parse should succeed");
        assert_eq!(1, result.len());
        assert_eq!(Some("cert-manager/cert-manager-webhook".to_string()), result[0].name);
        assert_eq!(Some("cert-manager".to_string()), result[0].namespace);
        assert_eq!(Some("MutatingWebhookConfiguration".to_string()), result[0].kind);
        assert_eq!(Some("admissionregistration.k8s.io/v1beta1".to_string()), result[0].api_version);
        assert_eq!(Some("admissionregistration.k8s.io/v1".to_string()), result[0].replace_with);
        assert_eq!(Some("1.16.0".to_string()), result[0].since);
        assert_eq!(Some("k8s".to_string()), result[0].rule_set);
    }

    #[test]
    fn test_parse_deprecations_target_versions_only_is_empty_result() {
        let payload = r#"
{
  "target-versions": {
    "k8s": "v1.33"
  }
}
"#;

        let result = parse_deprecations(payload).expect("Parse should succeed");
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_deprecations_missing_required_fields_is_rejected_in_strict_mode() {
        let payload = r#"
{
  "items": [
    {
      "name": "legacy-ingress",
      "api": {
        "version": "networking.k8s.io/v1beta1",
        "kind": "Ingress",
        "deprecated-in": "v1.19.0",
        "removed-in": "v1.22.0",
        "replacement-api": "networking.k8s.io/v1",
        "replacement-available-in": "v1.19.0",
        "component": "k8s"
      }
    }
  ],
  "target-versions": {
    "cert-manager": "v1.5.3",
    "istio": "v1.11.0",
    "k8s": "v1.35"
  }
}
"#;

        let result = parse_deprecations(payload);
        assert!(result.is_err());
    }

    #[test]
    fn test_is_expected_pluto_result_exit_status() {
        assert!(is_expected_pluto_result_exit_status(&RawCommandError::ExitStatusError(
            std::process::ExitStatus::from_raw(2 << 8),
        )));
        assert!(is_expected_pluto_result_exit_status(&RawCommandError::ExitStatusError(
            std::process::ExitStatus::from_raw(3 << 8),
        )));
        assert!(is_expected_pluto_result_exit_status(&RawCommandError::ExitStatusError(
            std::process::ExitStatus::from_raw(4 << 8),
        )));
        assert!(!is_expected_pluto_result_exit_status(&RawCommandError::ExitStatusError(
            std::process::ExitStatus::from_raw(1 << 8),
        )));
    }
}

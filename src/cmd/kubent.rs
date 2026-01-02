use crate::cmd::command::{CommandKiller, ExecutableCommand, QoveryCommand};
use crate::errors;
use crate::errors::CommandError;
use itertools::Itertools;
use serde::{Deserialize as SerdeDeserialize, Deserializer};
use serde_derive::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Deprecation {
    #[serde(rename = "Name")]
    #[serde(deserialize_with = "map_undefined_value")]
    pub name: Option<String>,
    #[serde(rename = "Namespace")]
    #[serde(deserialize_with = "map_undefined_value")]
    pub namespace: Option<String>,
    #[serde(rename = "Kind")]
    #[serde(deserialize_with = "map_undefined_value")]
    pub kind: Option<String>,
    #[serde(rename = "ApiVersion")]
    #[serde(deserialize_with = "map_undefined_value")]
    pub api_version: Option<String>,
    #[serde(rename = "RuleSet")]
    #[serde(deserialize_with = "map_undefined_value")]
    pub rule_set: Option<String>,
    #[serde(rename = "ReplaceWith")]
    #[serde(deserialize_with = "map_undefined_value")]
    pub replace_with: Option<String>,
    #[serde(rename = "Since")]
    #[serde(deserialize_with = "map_undefined_value")]
    pub since: Option<String>,
}

fn map_undefined_value<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt: Option<String> = Option::deserialize(deserializer)?;
    Ok(opt.and_then(|s| match s.as_str() {
        "<undefined>" => None,
        _ => Some(s),
    }))
}

#[derive(thiserror::Error, Clone, Debug, PartialEq)]
pub enum KubentError {
    #[error("Kubernetes config file path is not valid or does not exist: {kubeconfig_path}")]
    InvalidKubeConfig { kubeconfig_path: String },
    #[error("Kubent command terminated with an error: {error:?}")]
    CmdError { error: errors::CommandError },
    #[error("Kubent command generated an invalid output: {output}")]
    InvalidCmdOutputError { output: String },
}

#[derive(Clone)]
struct KubentCmdOutput {
    stdout: Option<String>,
}

/// This struct is used to wrap the kubent command and its output allowing for easier testing
/// Not meant to be exposed to the outside world.
#[cfg_attr(test, faux::create)]
struct KubentCmd {}

#[cfg_attr(test, faux::methods)]
impl<'m> KubentCmd {
    pub fn new() -> Self {
        Self {}
    }

    pub fn get_deprecations(
        &'m self,
        kubeconfig: &Path,
        target_version: Option<String>,
        envs: &'m [(&'m str, &'m str)],
    ) -> Result<KubentCmdOutput, CommandError> {
        let mut target_version_flag = "".to_string();
        let mut target_version_value = "".to_string();
        if let Some(target_version) = target_version {
            target_version_flag = "--target-version".to_string();
            target_version_value = target_version;
        }

        let args = &[
            "--kubeconfig",
            kubeconfig.to_str().unwrap_or_default(),
            "--output",
            "json",
            // "--exit-error", // returns a 200 status code even if there are deprecations which lead qovery command to consider it as failed
            "--log-level",
            "disabled",
            &target_version_flag,
            &target_version_value,
        ];

        let mut envs_with_soft_memory_limit = envs.to_vec();
        if !envs.iter().any(|(k, _v)| k == &"GOMEMLIMIT") {
            // Set a soft memory limit of 64MiB for kubent since it can eventually OOM
            // This is not a hard limit, it's just a hint to the Go runtime trying to keep
            // memory under the limit by triggering GC more often.
            envs_with_soft_memory_limit.push(("GOMEMLIMIT", "64MiB"));
        }

        let mut stdout_output: Vec<String> = Vec::new();
        let mut cmd = QoveryCommand::new("kubent", args, envs_with_soft_memory_limit.as_slice());
        let stdout_output_formatter = &mut |line| {
            stdout_output.push(line);
        };
        match cmd.exec_with_abort(
            stdout_output_formatter,
            &mut |line| warn!("kubent stderr: {}", line),
            &CommandKiller::from_timeout(Duration::from_secs(10 * 60)),
        ) {
            Ok(_) => Ok(KubentCmdOutput {
                stdout: match stdout_output.is_empty() {
                    true => None,
                    false => Some(stdout_output.iter().join("")),
                },
            }),
            Err(err) => Err(CommandError::new_from_legacy_command_error(
                err,
                Some("Cannot get deprecations from kubent".to_string()),
            )),
        }
    }
}

#[cfg_attr(test, faux::create)]
pub struct Kubent {
    kubent_cmd: KubentCmd,
}

#[cfg_attr(test, faux::methods)]
impl Default for Kubent {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg_attr(test, faux::methods)]
impl<'m> Kubent {
    pub fn new() -> Self {
        Self {
            kubent_cmd: KubentCmd::new(),
        }
    }

    /// This method is used for testing purposes only
    #[cfg(test)]
    fn new_with_kubent_cmd(kubent_cmd: KubentCmd) -> Self {
        Self { kubent_cmd }
    }

    pub fn get_deprecations(
        &'m self,
        kubeconfig: &Path,
        target_version: Option<String>,
        envs: &'m [(&'m str, &'m str)],
    ) -> Result<Vec<Deprecation>, KubentError> {
        if !kubeconfig.exists() {
            return Err(KubentError::InvalidKubeConfig {
                kubeconfig_path: kubeconfig.display().to_string(),
            });
        }

        match self.kubent_cmd.get_deprecations(kubeconfig, target_version, envs) {
            Ok(out) => Ok(match out.stdout {
                None => Vec::with_capacity(0),
                Some(ref stdout) => match stdout.is_empty() {
                    true => Vec::with_capacity(0),
                    false => serde_json::from_str(stdout)
                        .map_err(|e| KubentError::InvalidCmdOutputError { output: e.to_string() })?,
                },
            }),
            Err(err) => Err(KubentError::CmdError { error: err }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::CommandError;
    use std::{fs::File, path::PathBuf};
    use tempfile::tempdir;

    #[test]
    fn test_kubent_get_deprecations_invalid_kubeconfig() {
        // setup:
        let kubeconfig = PathBuf::from("/tmp/kubeconfig-this-one-doesnt-exist");

        let mut service_mock = KubentCmd::faux();
        faux::when!(service_mock.get_deprecations(_, _, _)).then_return(Ok(KubentCmdOutput {
            stdout: Some("[]".to_string()),
        }));
        let kubent = Kubent::new_with_kubent_cmd(service_mock);

        // execute:
        let deprecations = kubent.get_deprecations(&kubeconfig, None, &[]);

        // validate:
        assert!(deprecations.is_err());
        assert_eq!(
            KubentError::InvalidKubeConfig {
                kubeconfig_path: kubeconfig.display().to_string()
            },
            deprecations.expect_err("Error expected")
        );
    }

    #[test]
    fn test_kubent_get_deprecations_no_items() {
        // setup:
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let kubeconfig = temp_dir.path().join("kubeconfig");
        let _kubeconfig_file = File::create(&kubeconfig).expect("Failed to create kubeconfig file");
        let _created_temp_dir_guard = scopeguard::guard((), |_| {
            // delete temp dir after test
            std::fs::remove_dir_all(temp_dir.path()).expect("Failed to remove temp dir");
        });

        let mut service_mock = KubentCmd::faux();
        faux::when!(service_mock.get_deprecations(_, _, _)).then_return(Ok(KubentCmdOutput {
            stdout: Some("[]".to_string()),
        }));
        let kubent = Kubent::new_with_kubent_cmd(service_mock);

        // execute:
        let deprecations = kubent
            .get_deprecations(&kubeconfig, None, &[])
            .expect("Failed to get deprecations");

        // validate:
        assert!(deprecations.is_empty());
    }

    #[test]
    fn test_kubent_get_deprecations_some_items() {
        // setup:
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let kubeconfig = temp_dir.path().join("kubeconfig");
        let _kubeconfig_file = File::create(&kubeconfig).expect("Failed to create kubeconfig file");
        let _created_temp_dir_guard = scopeguard::guard((), |_| {
            // delete temp dir after test
            std::fs::remove_dir_all(temp_dir.path()).expect("Failed to remove temp dir");
        });

        let mut service_mock = KubentCmd::faux();
        faux::when!(service_mock.get_deprecations(_, _, _)).then_return(Ok(KubentCmdOutput {
            stdout: Some(
                ["[".to_string(),
                    r#"{"Name":"pod-identity-webhook","Namespace":"<undefined>","Kind":"MutatingWebhookConfiguration","ApiVersion":"admissionregistration.k8s.io/v1beta1","RuleSet":"Deprecated APIs removed in 1.22","ReplaceWith":"admissionregistration.k8s.io/v1","Since":"1.16.0"},"#.to_string(),
                    r#"{"Name":"eniconfigs.crd.k8s.amazonaws.com","Namespace":"<undefined>","Kind":"CustomResourceDefinition","ApiVersion":"apiextensions.k8s.io/v1beta1","RuleSet":"Deprecated APIs removed in 1.22","ReplaceWith":"apiextensions.k8s.io/v1","Since":"1.16.0"},"#.to_string(),
                    r#"{"Name":"grafana","Namespace":"<undefined>","Kind":"PodSecurityPolicy","ApiVersion":"policy/v1beta1","RuleSet":"Deprecated APIs removed in 1.25","ReplaceWith":"<removed>","Since":"1.21.0"},"#.to_string(),
                    r#"{"Name":"grafana-prod","Namespace":"<undefined>","Kind":"PodSecurityPolicy","ApiVersion":"policy/v1beta1","RuleSet":"Deprecated APIs removed in 1.25","ReplaceWith":"<removed>","Since":"1.21.0"},"#.to_string(),
                    r#"{"Name":"grafana-prod-test","Namespace":"<undefined>","Kind":"PodSecurityPolicy","ApiVersion":"policy/v1beta1","RuleSet":"Deprecated APIs removed in 1.25","ReplaceWith":"<removed>","Since":"1.21.0"},"#.to_string(),
                    r#"{"Name":"grafana-test","Namespace":"<undefined>","Kind":"PodSecurityPolicy","ApiVersion":"policy/v1beta1","RuleSet":"Deprecated APIs removed in 1.25","ReplaceWith":"<removed>","Since":"1.21.0"}"#.to_string(),
                "]".to_string(),
                ].join("")),
        }));
        let kubent = Kubent::new_with_kubent_cmd(service_mock);

        // execute:
        let deprecations = kubent
            .get_deprecations(&kubeconfig, None, &[])
            .expect("Failed to get deprecations");

        // validate:
        assert_eq!(6, deprecations.len());
        assert_eq!(
            vec![
                Deprecation {
                    name: Some("pod-identity-webhook".to_string()),
                    namespace: None,
                    kind: Some("MutatingWebhookConfiguration".to_string()),
                    api_version: Some("admissionregistration.k8s.io/v1beta1".to_string()),
                    rule_set: Some("Deprecated APIs removed in 1.22".to_string()),
                    replace_with: Some("admissionregistration.k8s.io/v1".to_string()),
                    since: Some("1.16.0".to_string()),
                },
                Deprecation {
                    name: Some("eniconfigs.crd.k8s.amazonaws.com".to_string()),
                    namespace: None,
                    kind: Some("CustomResourceDefinition".to_string()),
                    api_version: Some("apiextensions.k8s.io/v1beta1".to_string()),
                    rule_set: Some("Deprecated APIs removed in 1.22".to_string()),
                    replace_with: Some("apiextensions.k8s.io/v1".to_string()),
                    since: Some("1.16.0".to_string()),
                },
                Deprecation {
                    name: Some("grafana".to_string()),
                    namespace: None,
                    kind: Some("PodSecurityPolicy".to_string()),
                    api_version: Some("policy/v1beta1".to_string()),
                    rule_set: Some("Deprecated APIs removed in 1.25".to_string()),
                    replace_with: Some("<removed>".to_string()),
                    since: Some("1.21.0".to_string()),
                },
                Deprecation {
                    name: Some("grafana-prod".to_string()),
                    namespace: None,
                    kind: Some("PodSecurityPolicy".to_string()),
                    api_version: Some("policy/v1beta1".to_string()),
                    rule_set: Some("Deprecated APIs removed in 1.25".to_string()),
                    replace_with: Some("<removed>".to_string()),
                    since: Some("1.21.0".to_string()),
                },
                Deprecation {
                    name: Some("grafana-prod-test".to_string()),
                    namespace: None,
                    kind: Some("PodSecurityPolicy".to_string()),
                    api_version: Some("policy/v1beta1".to_string()),
                    rule_set: Some("Deprecated APIs removed in 1.25".to_string()),
                    replace_with: Some("<removed>".to_string()),
                    since: Some("1.21.0".to_string()),
                },
                Deprecation {
                    name: Some("grafana-test".to_string()),
                    namespace: None,
                    kind: Some("PodSecurityPolicy".to_string()),
                    api_version: Some("policy/v1beta1".to_string()),
                    rule_set: Some("Deprecated APIs removed in 1.25".to_string()),
                    replace_with: Some("<removed>".to_string()),
                    since: Some("1.21.0".to_string()),
                },
            ],
            deprecations
        );
    }

    #[test]
    fn test_kubent_get_deprecations_cmd_error() {
        // setup:
        let temp_dir = tempdir().expect("Failed to create temp dir");
        let kubeconfig = temp_dir.path().join("kubeconfig");
        let _kubeconfig_file = File::create(&kubeconfig).expect("Failed to create kubeconfig file");
        let _created_temp_dir_guard = scopeguard::guard((), |_| {
            // delete temp dir after test
            std::fs::remove_dir_all(temp_dir.path()).expect("Failed to remove temp dir");
        });

        let mut service_mock = KubentCmd::faux();
        faux::when!(service_mock.get_deprecations(_, _, _))
            .then_return(Err(CommandError::new_from_safe_message("kubent command failed".to_string())));
        let kubent = Kubent::new_with_kubent_cmd(service_mock);

        // execute:
        let deprecations_err = kubent
            .get_deprecations(&kubeconfig, None, &[])
            .expect_err("Command should be an error");

        // validate:
        assert_eq!(
            KubentError::CmdError {
                error: CommandError::new_from_safe_message("kubent command failed".to_string())
            },
            deprecations_err
        );
    }
}

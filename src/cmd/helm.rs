use std::io::{Error, Write};
use std::path::{Path, PathBuf};

use tracing::{error, info};

use crate::cloud_provider::helm::ChartInfo;
use crate::cmd::command::QoveryCommand;
use crate::cmd::helm::HelmCommand::{LIST, ROLLBACK, STATUS, UNINSTALL, UPGRADE};
use crate::cmd::helm::HelmError::{CannotRollback, CmdError, InvalidKubeConfig, ReleaseDoesNotExist};
use crate::cmd::structs::{HelmChart, HelmListItem};
use crate::errors::{CommandError, EngineError};
use crate::events::EventDetails;
use semver::Version;
use serde_derive::Deserialize;
use std::fs::File;
use std::str::FromStr;

const HELM_DEFAULT_TIMEOUT_IN_SECONDS: u32 = 600;
const HELM_MAX_HISTORY: &str = "50";

pub enum Timeout<T> {
    Default,
    Value(T),
}

impl Timeout<u32> {
    pub fn value(&self) -> u32 {
        match *self {
            Timeout::Default => HELM_DEFAULT_TIMEOUT_IN_SECONDS,
            Timeout::Value(t) => t,
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum HelmError {
    #[error("Kubernetes config file path is not valid or does not exist: {0}")]
    InvalidKubeConfig(PathBuf),

    #[error("Requested Helm release `{0}` does not exist")]
    ReleaseDoesNotExist(String),

    #[error("Requested Helm release `{0}` is under an helm lock. Ensure release is de-locked before going further")]
    ReleaseLocked(String),

    #[error("Helm release `{0}` during helm {1:?} has been rollbacked")]
    Rollbacked(String, HelmCommand),

    #[error("Helm release `{0}` cannot be rollbacked due to be at revision 1")]
    CannotRollback(String),

    #[error("Helm timed out for release `{0}` during helm {1:?}: {2}")]
    Timeout(String, HelmCommand, String),

    #[error("Helm command `{1:?}` for release {0} terminated with an error: {2:?}")]
    CmdError(String, HelmCommand, CommandError),
}

#[derive(Debug)]
pub struct Helm {
    kubernetes_config: PathBuf,
    common_envs: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy)]
pub enum HelmCommand {
    ROLLBACK,
    STATUS,
    UPGRADE,
    UNINSTALL,
    LIST,
    DIFF,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ReleaseInfo {
    // https://github.com/helm/helm/blob/12f1bc0acdeb675a8c50a78462ed3917fb7b2e37/pkg/release/status.go
    status: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ReleaseStatus {
    pub version: u64,
    pub info: ReleaseInfo,
}

impl ReleaseStatus {
    fn is_locked(&self) -> bool {
        self.info.status.starts_with("pending-")
    }
}

impl Helm {
    fn get_all_envs<'a>(&'a self, envs: &'a [(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
        let mut all_envs: Vec<(&str, &str)> = self.common_envs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        all_envs.append(&mut envs.to_vec());

        all_envs
    }

    pub fn new<P: AsRef<Path>>(kubernetes_config: P, common_envs: &[(&str, &str)]) -> Result<Helm, HelmError> {
        // Check kube config file is valid
        let kubernetes_config = kubernetes_config.as_ref().to_path_buf();
        if !kubernetes_config.exists() || !kubernetes_config.is_file() {
            return Err(InvalidKubeConfig(kubernetes_config));
        }

        Ok(Helm {
            kubernetes_config,
            common_envs: common_envs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        })
    }

    pub fn check_release_exist(&self, chart: &ChartInfo, envs: &[(&str, &str)]) -> Result<ReleaseStatus, HelmError> {
        let namespace = chart.get_namespace_string();
        let args = vec![
            "status",
            &chart.name,
            "--kubeconfig",
            self.kubernetes_config.to_str().unwrap_or_default(),
            "--namespace",
            &namespace,
            "-o",
            "json",
        ];

        let mut stdout = String::new();
        let mut stderr = String::new();
        match helm_exec_with_output(
            &args,
            &self.get_all_envs(envs),
            &mut |line| stdout.push_str(&line),
            &mut |line| stderr.push_str(&line),
        ) {
            Err(_) if stderr.contains("release: not found") => Err(ReleaseDoesNotExist(chart.name.clone())),
            Err(err) => {
                stderr.push_str(&err.message());
                let error = CommandError::new(stderr, err.message_safe());
                Err(CmdError(chart.name.clone(), STATUS, error))
            }
            Ok(_) => {
                let status: ReleaseStatus = serde_json::from_str(&stdout).unwrap_or_default();
                Ok(status)
            }
        }
    }

    pub fn rollback(&self, chart: &ChartInfo, envs: &[(&str, &str)]) -> Result<(), HelmError> {
        if self.check_release_exist(chart, envs)?.version <= 1 {
            return Err(CannotRollback(chart.name.clone()));
        }

        let timeout = format!("{}s", &chart.timeout_in_seconds);
        let namespace = chart.get_namespace_string();
        let args = vec![
            "rollback",
            &chart.name,
            "--kubeconfig",
            self.kubernetes_config.to_str().unwrap_or_default(),
            "--namespace",
            &namespace,
            "--timeout",
            &timeout,
            "--history-max",
            HELM_MAX_HISTORY,
            "--cleanup-on-fail",
            "--force",
            "--wait",
        ];

        let mut stderr = String::new();
        match helm_exec_with_output(&args, &self.get_all_envs(envs), &mut |_| {}, &mut |line| stderr.push_str(&line)) {
            Err(err) => {
                stderr.push_str(&err.message());
                let error = CommandError::new(stderr, err.message_safe());
                Err(CmdError(chart.name.clone(), ROLLBACK, error))
            }
            Ok(_) => Ok(()),
        }
    }

    pub fn uninstall(&self, chart: &ChartInfo, envs: &[(&str, &str)]) -> Result<(), HelmError> {
        // If the release does not exist, we do not return an error
        match self.check_release_exist(chart, envs) {
            Ok(_) => {}
            Err(ReleaseDoesNotExist(_)) => return Ok(()),
            Err(err) => return Err(err),
        }

        let timeout = format!("{}s", &chart.timeout_in_seconds);
        let namespace = chart.get_namespace_string();
        let args = vec![
            "uninstall",
            &chart.name,
            "--kubeconfig",
            self.kubernetes_config.to_str().unwrap_or_default(),
            "--namespace",
            &namespace,
            "--timeout",
            &timeout,
            "--wait",
        ];

        let mut stderr = String::new();
        match helm_exec_with_output(&args, &self.get_all_envs(envs), &mut |_| {}, &mut |line| stderr.push_str(&line)) {
            Err(err) => {
                stderr.push_str(&err.message());
                let error = CommandError::new(stderr, err.message_safe());
                Err(CmdError(chart.name.clone(), UNINSTALL, error))
            }
            Ok(_) => Ok(()),
        }
    }

    fn unlock_release(&self, chart: &ChartInfo, envs: &[(&str, &str)]) -> Result<(), HelmError> {
        match self.check_release_exist(chart, envs) {
            Ok(release) if release.is_locked() && release.version <= 1 => {
                info!("Helm lock detected. Uninstalling it as it is the first version and rollback is not possible");
                self.uninstall(chart, envs)?;
            }
            Ok(release) if release.is_locked() => {
                info!("Helm lock detected. Forcing rollback to previous version");
                self.rollback(chart, envs)?;
            }
            Ok(release) => {
                // Happy path nothing to do
                debug!("Helm release status: {:?}", release)
            }
            Err(_) => {} // Happy path nothing to do
        }

        Ok(())
    }

    /// List deployed helm charts
    ///
    /// # Arguments
    ///
    /// * `envs` - environment variables required for kubernetes connection
    /// * `namespace` - list charts from a kubernetes namespace or use None to select all namespaces
    pub fn list_release(&self, namespace: Option<&str>, envs: &[(&str, &str)]) -> Result<Vec<HelmChart>, HelmError> {
        let mut helm_args = vec![
            "list",
            "-a",
            "--kubeconfig",
            self.kubernetes_config.to_str().unwrap_or_default(),
            "-o",
            "json",
        ];
        match namespace {
            Some(ns) => helm_args.append(&mut vec!["-n", ns]),
            None => helm_args.push("-A"),
        }

        let mut output_string: Vec<String> = Vec::with_capacity(20);
        if let Err(cmd_error) = helm_exec_with_output(
            &helm_args,
            &self.get_all_envs(envs),
            &mut |line| output_string.push(line),
            &mut |line| error!("{}", line),
        ) {
            return Err(HelmError::CmdError("none".to_string(), LIST, cmd_error));
        }

        let values = serde_json::from_str::<Vec<HelmListItem>>(&output_string.join(""));
        let mut helms_charts: Vec<HelmChart> = Vec::new();

        match values {
            Ok(all_helms) => {
                for helm in all_helms {
                    let last_dash_pos = helm.chart.rfind('-').expect("Can't parse helm chart") + 1;
                    let raw_version = helm.chart[last_dash_pos..].to_string();
                    let version = Version::from_str(raw_version.as_str()).ok();
                    helms_charts.push(HelmChart::new(helm.name, helm.namespace, version))
                }

                Ok(helms_charts)
            }
            Err(e) => {
                let message_safe = "Error while deserializing all helms names";
                Err(HelmError::CmdError(
                    "none".to_string(),
                    LIST,
                    CommandError::new(format!("{}, error: {}", message_safe, e), Some(message_safe.to_string())),
                ))
            }
        }
    }

    pub fn get_chart_version(
        &self,
        chart_name: String,
        namespace: Option<&str>,
        envs: &[(&str, &str)],
    ) -> Result<Option<Version>, HelmError> {
        let deployed_charts = self.list_release(namespace, envs)?;
        for chart in deployed_charts {
            if chart.name == chart_name {
                return Ok(chart.version);
            }
        }

        // found nothing ;'(
        Ok(None)
    }

    pub fn upgrade_diff(&self, chart: &ChartInfo, envs: &[(&str, &str)]) -> Result<(), HelmError> {
        let mut args_string: Vec<String> = vec![
            "diff".to_string(),
            "upgrade".to_string(),
            "--kubeconfig".to_string(),
            self.kubernetes_config.to_str().unwrap_or_default().to_string(),
            "--install".to_string(),
            "--namespace".to_string(),
            chart.get_namespace_string(),
        ];

        for value in &chart.values {
            args_string.push("--set".to_string());
            args_string.push(format!("{}={}", value.key, value.value));
        }

        for value_file in &chart.values_files {
            args_string.push("-f".to_string());
            args_string.push(value_file.clone());
        }

        for value_file in &chart.yaml_files_content {
            let file_path = format!("{}/{}", chart.path, &value_file.filename);
            let file_create = || -> Result<(), Error> {
                let mut file = File::create(&file_path)?;
                file.write_all(value_file.yaml_content.as_bytes())?;
                Ok(())
            };

            // no need to validate yaml as it will be done by helm
            if let Err(e) = file_create() {
                let safe_message = format!("Error while writing yaml content to file `{}`", &file_path);
                let cmd_err = CommandError::new(
                    format!("{}\nContent\n{}\nError: {}", safe_message, value_file.yaml_content, e),
                    Some(safe_message),
                );
                return Err(HelmError::CmdError(chart.name.clone(), HelmCommand::UPGRADE, cmd_err));
            };

            args_string.push("-f".to_string());
            args_string.push(file_path);
        }

        // add last elements
        args_string.push(chart.name.clone());
        args_string.push(chart.path.clone());

        let mut stderr_msg = String::new();
        let helm_ret = helm_exec_with_output(
            &args_string.iter().map(|x| x.as_str()).collect::<Vec<&str>>(),
            &self.get_all_envs(envs),
            &mut |line| {
                debug!("{}", line);
            },
            &mut |line| {
                stderr_msg.push_str(&line);
                warn!("chart {}: {}", chart.name, line);
            },
        );

        match helm_ret {
            // Ok is ok
            Ok(_) => Ok(()),
            Err(err) => {
                error!("Helm error: {:?}", err);
                Err(CmdError(
                    chart.name.clone(),
                    HelmCommand::DIFF,
                    CommandError::new(stderr_msg.clone(), Some(stderr_msg)),
                ))
            }
        }
    }

    pub fn upgrade(&self, chart: &ChartInfo, envs: &[(&str, &str)]) -> Result<(), HelmError> {
        // Due to crash or error it is possible that the release is under an helm lock
        // Try to un-stuck the situation first if needed
        // We don't care if the rollback failed, as it is a best effort to remove the lock
        // and to re-launch an upgrade just after
        let unlock_ret = self.unlock_release(chart, envs);
        info!("Helm lock status: {:?}", unlock_ret);

        let debug = false;
        let timeout_string = format!("{}s", &chart.timeout_in_seconds);

        let mut args_string: Vec<String> = vec![
            "upgrade".to_string(),
            "--kubeconfig".to_string(),
            self.kubernetes_config.to_str().unwrap_or_default().to_string(),
            "--create-namespace".to_string(),
            "--install".to_string(),
            "--timeout".to_string(),
            timeout_string.as_str().to_string(),
            "--history-max".to_string(),
            HELM_MAX_HISTORY.to_string(),
            "--namespace".to_string(),
            chart.get_namespace_string(),
        ];

        if debug {
            args_string.push("-o".to_string());
            args_string.push("json".to_string());
        }

        // warn: don't add debug or json output won't work
        if chart.atomic {
            args_string.push("--atomic".to_string())
        }
        if chart.force_upgrade {
            args_string.push("--force".to_string())
        }
        if chart.dry_run {
            args_string.push("--dry-run".to_string())
        }
        if chart.wait {
            args_string.push("--wait".to_string())
        }

        // overrides and files overrides
        for value in &chart.values {
            args_string.push("--set".to_string());
            args_string.push(format!("{}={}", value.key, value.value));
        }

        for value_file in &chart.values_files {
            args_string.push("-f".to_string());
            args_string.push(value_file.clone());
        }
        for value_file in &chart.yaml_files_content {
            let file_path = format!("{}/{}", chart.path, &value_file.filename);
            let file_create = || -> Result<(), Error> {
                let mut file = File::create(&file_path)?;
                file.write_all(value_file.yaml_content.as_bytes())?;
                Ok(())
            };

            // no need to validate yaml as it will be done by helm
            if let Err(e) = file_create() {
                let safe_message = format!("Error while writing yaml content to file `{}`", &file_path);
                let cmd_err = CommandError::new(
                    format!("{}\nContent\n{}\nError: {}", safe_message, value_file.yaml_content, e),
                    Some(safe_message),
                );
                return Err(HelmError::CmdError(chart.name.clone(), HelmCommand::UPGRADE, cmd_err));
            };

            args_string.push("-f".to_string());
            args_string.push(file_path);
        }

        // add last elements
        args_string.push(chart.name.clone());
        args_string.push(chart.path.clone());

        let mut error_message: Vec<String> = vec![];

        let helm_ret = helm_exec_with_output(
            &args_string.iter().map(|x| x.as_str()).collect::<Vec<&str>>(),
            &self.get_all_envs(envs),
            &mut |line| {
                info!("{}", line);
            },
            &mut |line| {
                warn!("chart {}: {}", chart.name, line);
                error_message.push(line);
            },
        );

        match helm_ret {
            // Ok is ok
            Ok(_) => Ok(()),
            Err(err) => {
                error!("Helm error: {:?}", err);

                // Try do define/specify a bit more the message
                let stderr_msg: String = error_message.into_iter().collect();
                let stderr_msg = format!("{}: {}", stderr_msg, err.message());
                let error = if stderr_msg.contains("another operation (install/upgrade/rollback) is in progress") {
                    HelmError::ReleaseLocked(chart.name.clone())
                } else if stderr_msg.contains("has been rolled back") {
                    HelmError::Rollbacked(chart.name.clone(), UPGRADE)
                } else if stderr_msg.contains("timed out waiting") {
                    HelmError::Timeout(chart.name.clone(), UPGRADE, stderr_msg)
                } else {
                    CmdError(
                        chart.name.clone(),
                        HelmCommand::UPGRADE,
                        CommandError::new(stderr_msg.clone(), Some(stderr_msg)),
                    )
                };

                Err(error)
            }
        }
    }

    pub fn uninstall_chart_if_breaking_version(
        &self,
        chart: &ChartInfo,
        envs: &[(&str, &str)],
    ) -> Result<(), HelmError> {
        // If there is a breaking version set for the current helm chart,
        // then we compare this breaking version with the currently installed version if any.
        // If current installed version is older than breaking change one, then we delete
        // the chart before applying it.
        if let Some(breaking_version) = &chart.last_breaking_version_requiring_restart {
            if let Some(installed_version) =
                self.get_chart_version(chart.name.clone(), Some(chart.get_namespace_string().as_str()), envs)?
            {
                if installed_version.le(breaking_version) {
                    self.uninstall(chart, envs)?;
                }
            }
        }

        Ok(())
    }
}

fn helm_exec_with_output<STDOUT, STDERR>(
    args: &[&str],
    envs: &[(&str, &str)],
    stdout_output: &mut STDOUT,
    stderr_output: &mut STDERR,
) -> Result<(), CommandError>
where
    STDOUT: FnMut(String),
    STDERR: FnMut(String),
{
    // Note: Helm CLI use spf13/cobra lib for the CLI; One function is mainly used to return an error if a command failed.
    // Helm returns an error each time a command does not succeed as they want. Which leads to handling error with status code 1
    // It means that the command successfully ran, but it didn't terminate as expected
    let mut cmd = QoveryCommand::new("helm", args, envs);
    match cmd.exec_with_output(stdout_output, stderr_output) {
        Err(err) => Err(CommandError::new(format!("{:?}", err), None)),
        _ => Ok(()),
    }
}

pub fn to_command_error(error: HelmError) -> CommandError {
    CommandError::new_from_safe_message(error.to_string())
}

pub fn to_engine_error(event_details: &EventDetails, error: HelmError) -> EngineError {
    EngineError::new_helm_error(event_details.clone(), error)
}

#[cfg(feature = "test-with-kube")]
#[cfg(test)]
mod tests {
    use crate::cloud_provider::helm::{ChartInfo, ChartSetValue};
    use crate::cmd::command::QoveryCommand;
    use crate::cmd::helm::{helm_exec_with_output, Helm, HelmError};
    use semver::Version;
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::Duration;

    struct HelmTestCtx {
        helm: Helm,
        chart: ChartInfo,
    }

    impl HelmTestCtx {
        fn cleanup(&self) {
            let ret = self.helm.uninstall(&self.chart, &[]);
            assert!(ret.is_ok())
        }

        fn new(release_name: &str) -> HelmTestCtx {
            let chart = ChartInfo::new_from_custom_namespace(
                release_name.to_string(),
                "tests/helm/simple_nginx".to_string(),
                "default".to_string(),
                600,
                vec![],
                false,
                None,
            );
            let mut kube_config = dirs::home_dir().unwrap();
            kube_config.push(".kube/config");
            let helm = Helm::new(kube_config.to_str().unwrap(), &[]).unwrap();

            let cleanup = HelmTestCtx { helm, chart };
            cleanup.cleanup();
            cleanup
        }
    }

    impl Drop for HelmTestCtx {
        fn drop(&mut self) {
            self.cleanup()
        }
    }

    #[test]
    fn check_version() {
        let mut output = String::new();
        let _ = helm_exec_with_output(&["version"], &[], &mut |line| output.push_str(&line), &mut |_line| {});
        assert!(output.contains("Version:\"v3.7.2\""));
    }

    #[test]
    fn test_release_exist() {
        let HelmTestCtx { ref helm, ref chart } = HelmTestCtx::new("test-release-exist");
        let ret = helm.check_release_exist(chart, &[]);

        assert!(matches!(ret, Err(HelmError::ReleaseDoesNotExist(test)) if test == chart.name))
    }

    #[test]
    fn test_list_release() {
        let HelmTestCtx {
            ref helm,
            ref mut chart,
        } = HelmTestCtx::new("test-list-release");
        chart.custom_namespace = Some("hello-my-friend-this-is-a-test".to_string());

        // no existing namespace should return an empty array
        let ret = helm.list_release(Some("tsdfsfsdf"), &[]);
        assert!(matches!(ret, Ok(vec) if vec.is_empty()));

        // install something
        let ret = helm.upgrade(chart, &[]);
        assert!(matches!(ret, Ok(())));

        // We should have at least one release in all the release
        let ret = helm.list_release(None, &[]);
        assert!(matches!(ret, Ok(vec) if !vec.is_empty()));

        // We should have at least one release in all the release
        let ret = helm.list_release(Some(&chart.get_namespace_string()), &[]);
        assert!(matches!(ret, Ok(vec) if vec.len() == 1));

        // Install a second stuff
        let HelmTestCtx {
            ref helm,
            ref mut chart,
        } = HelmTestCtx::new("test-list-release-2");
        chart.custom_namespace = Some("hello-my-friend-this-is-a-test".to_string());
        let ret = helm.upgrade(chart, &[]);
        assert!(matches!(ret, Ok(())));

        let ret = helm.list_release(Some(&chart.get_namespace_string()), &[]);
        assert!(matches!(ret, Ok(vec) if vec.len() == 2));
    }

    #[test]
    fn test_upgrade_diff() {
        let HelmTestCtx { ref helm, ref chart } = HelmTestCtx::new("test-upgrade-diff");

        let ret = helm.upgrade_diff(chart, &[]);
        assert!(matches!(ret, Ok(())));
    }

    #[test]
    fn test_rollback() {
        let HelmTestCtx { ref helm, ref chart } = HelmTestCtx::new("test-rollback");

        // check release does not exist yet
        let ret = helm.rollback(chart, &[]);
        assert!(matches!(ret, Err(HelmError::ReleaseDoesNotExist(test)) if test == chart.name));

        // install it
        let ret = helm.upgrade(chart, &[]);
        assert!(matches!(ret, Ok(())));

        // First revision cannot be rollback
        let ret = helm.rollback(chart, &[]);
        assert!(matches!(ret, Err(HelmError::CannotRollback(_))));

        // 2nd upgrade
        let ret = helm.upgrade(chart, &[]);
        assert!(matches!(ret, Ok(())));

        // Rollback should be ok now
        let ret = helm.rollback(chart, &[]);
        assert!(matches!(ret, Ok(())));
    }

    #[test]
    fn test_upgrade() {
        let HelmTestCtx { ref helm, ref chart } = HelmTestCtx::new("test-upgrade");

        // check release does not exist yet
        let ret = helm.check_release_exist(chart, &[]);
        assert!(matches!(ret, Err(HelmError::ReleaseDoesNotExist(test)) if test == chart.name));

        // install it
        let ret = helm.upgrade(chart, &[]);
        assert!(matches!(ret, Ok(())));

        // check now it exists
        let ret = helm.check_release_exist(chart, &[]);
        assert!(matches!(ret, Ok(_)));
    }

    #[test]
    fn test_upgrade_timeout() {
        let HelmTestCtx {
            ref helm,
            ref mut chart,
        } = HelmTestCtx::new("test-upgrade-timeout");
        chart.timeout_in_seconds = 1;

        // check release does not exist yet
        let ret = helm.check_release_exist(chart, &[]);
        assert!(matches!(ret, Err(HelmError::ReleaseDoesNotExist(test)) if test == chart.name));

        // install it
        let ret = helm.upgrade(chart, &[]);
        assert!(matches!(ret, Err(HelmError::Timeout(_, _, _))));

        // Release should not exist if it fails
        let ret = helm.check_release_exist(chart, &[]);
        assert!(matches!(ret, Err(HelmError::ReleaseDoesNotExist(test)) if test == chart.name));
    }

    #[test]
    fn test_upgrade_with_lock_during_install() {
        // We want to check that we manage to install a chart even if a lock is present while it was the first installation
        let HelmTestCtx { ref helm, ref chart } = HelmTestCtx::new("test-upgrade-with-lock-install");

        // check release does not exist yet
        let ret = helm.check_release_exist(chart, &[]);
        assert!(matches!(ret, Err(HelmError::ReleaseDoesNotExist(test)) if test == chart.name));

        // Spawn our task killer
        let barrier = Arc::new(Barrier::new(2));
        std::thread::spawn({
            let barrier = barrier.clone();
            let chart_name = chart.name.clone();
            move || {
                barrier.wait();
                thread::sleep(Duration::from_millis(3000));
                let mut cmd = QoveryCommand::new("pkill", &["-9", "-f", &format!("helm.*{}", chart_name)], &[]);
                let _ = cmd.exec();
            }
        });

        // install it
        barrier.wait();
        let ret = helm.upgrade(chart, &[]);
        assert!(matches!(ret, Err(_)));

        // Release should be locked
        let ret = helm.check_release_exist(chart, &[]);
        assert!(matches!(ret, Ok(release) if release.is_locked()));

        // New installation should work even if a lock is present
        let ret = helm.upgrade(chart, &[]);
        assert!(matches!(ret, Ok(())));

        // Release should not be locked anymore
        let ret = helm.check_release_exist(chart, &[]);
        assert!(matches!(ret, Ok(release) if !release.is_locked()));
    }

    #[test]
    fn test_upgrade_with_lock_during_upgrade() {
        // We want to check that we manage to install a chart even if a lock is present while it not the first installation
        let HelmTestCtx {
            ref helm,
            ref mut chart,
        } = HelmTestCtx::new("test-upgrade-with-lock-upgrade");

        // check release does not exist yet
        let ret = helm.check_release_exist(chart, &[]);
        assert!(matches!(ret, Err(HelmError::ReleaseDoesNotExist(test)) if test == chart.name));

        // First install
        let ret = helm.upgrade(chart, &[]);
        assert!(matches!(ret, Ok(())));

        // Spawn our task killer
        let barrier = Arc::new(Barrier::new(2));
        std::thread::spawn({
            let barrier = barrier.clone();
            let chart_name = chart.name.clone();
            move || {
                barrier.wait();
                thread::sleep(Duration::from_millis(3000));
                let mut cmd = QoveryCommand::new("pkill", &["-9", "-f", &format!("helm.*{}", chart_name)], &[]);
                let _ = cmd.exec();
            }
        });

        chart.values = vec![ChartSetValue {
            key: "initialDelaySeconds".to_string(),
            value: "6".to_string(),
        }];
        barrier.wait();
        let ret = helm.upgrade(chart, &[]);
        assert!(matches!(ret, Err(_)));

        // Release should be locked
        let ret = helm.check_release_exist(chart, &[]);
        assert!(matches!(ret, Ok(release) if release.is_locked() && release.version == 2));

        // New installation should work even if a lock is present
        let ret = helm.upgrade(chart, &[]);
        assert!(matches!(ret, Ok(())));

        // Release should not be locked anymore
        let ret = helm.check_release_exist(chart, &[]);
        assert!(matches!(ret, Ok(release) if !release.is_locked() && release.version == 4));
    }

    #[test]
    fn test_uninstall() {
        let HelmTestCtx { ref helm, ref chart } = HelmTestCtx::new("test-uninstall");

        // check release does not exist yet
        let ret = helm.check_release_exist(chart, &[]);
        assert!(matches!(ret, Err(HelmError::ReleaseDoesNotExist(test)) if test == chart.name));

        // deleting something that does not exist should not be an issue
        let ret = helm.uninstall(chart, &[]);
        assert!(matches!(ret, Ok(())));

        // install it
        let ret = helm.upgrade(chart, &[]);
        assert!(matches!(ret, Ok(())));

        // check now it exists
        let ret = helm.check_release_exist(chart, &[]);
        assert!(matches!(ret, Ok(_)));

        // Delete it
        let ret = helm.uninstall(chart, &[]);
        assert!(matches!(ret, Ok(())));

        // check release does not exist anymore
        let ret = helm.check_release_exist(chart, &[]);
        assert!(matches!(ret, Err(HelmError::ReleaseDoesNotExist(test)) if test == chart.name));
    }

    #[test]
    fn test_getting_version() {
        let HelmTestCtx {
            ref helm,
            ref mut chart,
        } = HelmTestCtx::new("test-version-release");
        let _ = helm.upgrade(chart, &[]);
        let releases = helm.list_release(Some(&chart.get_namespace_string()), &[]).unwrap();
        assert_eq!(releases[0].clone().version.unwrap(), Version::new(0, 1, 0))
    }
}

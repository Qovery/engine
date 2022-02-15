use std::io::{Error, Write};
use std::path::{Path, PathBuf};

use tracing::{error, info, span, Level};

use crate::cloud_provider::helm::ChartInfo;
use crate::cmd::helm::HelmCommand::{ROLLBACK, STATUS, UNINSTALL, UPGRADE};
use crate::cmd::helm::HelmError::{CannotRollback, CmdError, InvalidKubeConfig, ReleaseDoesNotExist};
use crate::cmd::structs::{HelmChart, HelmHistoryRow, HelmListItem};
use crate::cmd::utilities::QoveryCommand;
use crate::error::{SimpleError, SimpleErrorKind};
use crate::errors::{CommandError, EngineError};
use crate::events::EventDetails;
use chrono::Duration;
use retry::delay::Fixed;
use retry::Error::Operation;
use retry::OperationResult;
use semver::Version;
use serde_derive::Deserialize;
use std::fs::File;
use std::str::FromStr;

const HELM_DEFAULT_TIMEOUT_IN_SECONDS: u32 = 300;
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

    #[error("Helm command `{0:?}` terminated with an error: {1:?}")]
    CmdError(HelmCommand, CommandError),
}

#[derive(Debug)]
pub struct Helm {
    kubernetes_config: PathBuf,
}

#[derive(Debug, Clone, Copy)]
pub enum HelmCommand {
    ROLLBACK,
    STATUS,
    UPGRADE,
    UNINSTALL,
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
    pub fn new<P: AsRef<Path>>(kubernetes_config: P) -> Result<Helm, HelmError> {
        // Check kube config file is valid
        let kubernetes_config = kubernetes_config.as_ref().to_path_buf();
        if !kubernetes_config.exists() || !kubernetes_config.is_file() {
            return Err(InvalidKubeConfig(kubernetes_config));
        }

        Ok(Helm { kubernetes_config })
    }

    pub fn check_release_exist(&self, chart: &ChartInfo) -> Result<ReleaseStatus, HelmError> {
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
            args,
            vec![],
            |line| stdout.push_str(&line),
            |line| stderr.push_str(&line),
        ) {
            Err(_) if stderr.contains("Error: release: not found") => Err(ReleaseDoesNotExist(chart.name.clone())),
            Err(err) => {
                stderr.push_str(&err.message());
                let error = CommandError::new(stderr, err.message_safe());
                Err(CmdError(STATUS, error))
            }
            Ok(_) => {
                let status: ReleaseStatus = serde_json::from_str(&stdout).unwrap_or_default();
                Ok(status)
            }
        }
    }

    pub fn rollback(&self, chart: &ChartInfo) -> Result<(), HelmError> {
        if self.check_release_exist(chart)?.version <= 1 {
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
        match helm_exec_with_output(args, vec![], |_| {}, |line| stderr.push_str(&line)) {
            Err(err) => {
                stderr.push_str(&err.message());
                let error = CommandError::new(stderr, err.message_safe());
                Err(CmdError(ROLLBACK, error))
            }
            Ok(_) => Ok(()),
        }
    }

    pub fn uninstall(&self, chart: &ChartInfo) -> Result<(), HelmError> {
        // If the release does not exist, we do not return an error
        match self.check_release_exist(chart) {
            Ok(_) => {}
            Err(ReleaseDoesNotExist(_)) => return Ok(()),
            Err(err) => return Err(err),
        }

        let timeout = format!("{}s", &chart.timeout_in_seconds);
        let namespace = chart.get_namespace_string();
        let args = vec![
            "delete",
            &chart.name,
            "--kubeconfig",
            self.kubernetes_config.to_str().unwrap_or_default(),
            "--namespace",
            &namespace,
            "--timeout",
            &timeout,
            "--wait",
        ];

        match helm_exec(args, vec![]) {
            Err(err) => Err(CmdError(UNINSTALL, err)),
            Ok(_) => Ok(()),
        }
    }

    fn unlock_release(&self, chart: &ChartInfo) -> Result<(), HelmError> {
        match self.check_release_exist(chart) {
            Ok(release) if release.is_locked() && release.version <= 1 => {
                info!("Helm lock detected. Uninstalling it as it is the first version and rollback is not possible");
                self.uninstall(chart)?;
            }
            Ok(release) if release.is_locked() => {
                info!("Helm lock detected. Forcing rollback to previous version");
                self.rollback(chart)?;
            }
            Ok(release) => {
                // Happy path nothing to do
                debug!("Helm release status: {:?}", release)
            }
            Err(_) => {} // Happy path nothing to do
        }

        Ok(())
    }

    pub fn upgrade(&self, chart: &ChartInfo, envs: &[(&str, &str)]) -> Result<(), HelmError> {
        // Due to crash or error it is possible that the release is under an helm lock
        // Try to un-stuck the situation first if needed
        // We don't care if the rollback failed, as it is a best effort to remove the lock
        // and to re-launch an upgrade just after
        let unlock_ret = self.unlock_release(chart);
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
                return Err(HelmError::CmdError(HelmCommand::UPGRADE, cmd_err));
            };

            args_string.push("-f".to_string());
            args_string.push(file_path);
        }

        // add last elements
        args_string.push(chart.name.clone());
        args_string.push(chart.path.clone());

        let mut error_message: Vec<String> = vec![];

        let helm_ret = helm_exec_with_output(
            args_string.iter().map(|x| x.as_str()).collect(),
            envs.to_vec(),
            |line| {
                info!("{}", line);
            },
            |line| {
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
                        HelmCommand::UPGRADE,
                        CommandError::new(stderr_msg.clone(), Some(stderr_msg)),
                    )
                };

                Err(error)
            }
        }
    }
}

pub fn helm_destroy_chart_if_breaking_changes_version_detected(
    kubernetes_config: &Path,
    environment_variables: &Vec<(&str, &str)>,
    chart_info: &ChartInfo,
) -> Result<(), CommandError> {
    // If there is a breaking version set for the current helm chart,
    // then we compare this breaking version with the currently installed version if any.
    // If current installed version is older than breaking change one, then we delete
    // the chart before applying it.
    if let Some(breaking_version) = &chart_info.last_breaking_version_requiring_restart {
        let chart_namespace = chart_info.get_namespace_string();
        if let Some(installed_version) = helm_get_chart_version(
            kubernetes_config,
            environment_variables.to_owned(),
            Some(chart_namespace.as_str()),
            chart_info.name.clone(),
        ) {
            if installed_version.le(breaking_version) {
                // FIXME: xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
                let helm = Helm::new(kubernetes_config).unwrap();
                helm.uninstall(&chart_info).unwrap();
            }
        }
    }

    Ok(())
}

pub fn helm_exec_upgrade_with_chart_info<P>(
    kubernetes_config: P,
    envs: &Vec<(&str, &str)>,
    chart: &ChartInfo,
) -> Result<(), CommandError>
where
    P: AsRef<Path>,
{
    let debug = false;
    let timeout_string = format!("{}s", &chart.timeout_in_seconds);

    let mut args_string: Vec<String> = vec![
        "upgrade",
        "--kubeconfig",
        kubernetes_config.as_ref().to_str().unwrap(),
        "--create-namespace",
        "--install",
        "--timeout",
        timeout_string.as_str(),
        "--history-max",
        "50",
        "--namespace",
        chart.get_namespace_string().as_str(),
    ]
    .into_iter()
    .map(|x| x.to_string())
    .collect();

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
            return Err(CommandError::new(
                format!(
                    "{}\nContent\n{}\nError: {}",
                    safe_message.to_string(),
                    value_file.yaml_content,
                    e
                ),
                Some(safe_message.to_string()),
            ));
        };

        args_string.push("-f".to_string());
        args_string.push(file_path.clone());
    }

    // add last elements
    args_string.push(chart.name.to_string());
    args_string.push(chart.path.to_string());

    let mut json_output_string = String::new();
    let mut error_message = String::new();

    let result = retry::retry(Fixed::from_millis(15000).take(3), || {
        let args = args_string.iter().map(|x| x.as_str()).collect();
        let mut helm_error_during_deployment = SimpleError {
            kind: SimpleErrorKind::Other,
            message: None,
        };
        let mut should_clean_helm_lock = false;

        let helm_ret = helm_exec_with_output(
            args,
            envs.clone(),
            |line| {
                info!("{}", line);
                json_output_string = line
            },
            |line| {
                if line.contains("another operation (install/upgrade/rollback) is in progress") {
                    error_message = format!("helm lock detected for {}, looking for cleaning lock", chart.name);
                    helm_error_during_deployment.message = Some(error_message.clone());
                    warn!("{}. {}", &error_message, &line);
                    should_clean_helm_lock = true;
                    return;
                }

                if !chart.parse_stderr_for_error {
                    warn!("chart {}: {}", chart.name, line);
                    return;
                }

                // helm errors are not json formatted unfortunately
                if line.contains("has been rolled back") {
                    error_message = format!("deployment {} has been rolled back", chart.name);
                    helm_error_during_deployment.message = Some(error_message.clone());
                    warn!("{}. {}", &error_message, &line);
                } else if line.contains("has been uninstalled") {
                    error_message = format!("deployment {} has been uninstalled due to failure", chart.name);
                    helm_error_during_deployment.message = Some(error_message.clone());
                    warn!("{}. {}", &error_message, &line);
                    // special fix for prometheus operator
                } else if line.contains("info: skipping unknown hook: \"crd-install\"") {
                    debug!("chart {}: {}", chart.name, line);
                } else {
                    error_message = format!("deployment {} has failed", chart.name);
                    helm_error_during_deployment.message = Some(error_message.clone());
                    error!("{}. {}", &error_message, &line);
                }
            },
        );

        match helm_ret {
            Ok(_) => {
                if helm_error_during_deployment.message.is_some() {
                    OperationResult::Retry(helm_error_during_deployment)
                } else {
                    OperationResult::Ok(())
                }
            }
            Err(e) => OperationResult::Retry(SimpleError::new(SimpleErrorKind::Other, Some(e.message()))),
        }
    });

    match result {
        Ok(_) => Ok(()),
        Err(Operation { error, .. }) => {
            return Err(CommandError::new(
                error.message.unwrap_or("No error message".to_string()),
                None,
            ));
        }
        Err(retry::Error::Internal(e)) => return Err(CommandError::new(e, None)),
    }
}

pub enum HelmDeploymentErrors {
    SimpleError,
    HelmLockError,
}

pub fn helm_exec_history<P>(
    kubernetes_config: P,
    namespace: &str,
    release_name: &str,
    envs: &Vec<(&str, &str)>,
) -> Result<Vec<HelmHistoryRow>, CommandError>
where
    P: AsRef<Path>,
{
    let mut output_string = String::new();
    let _ = helm_exec_with_output(
        // WARN: do not add argument --debug, otherwise JSON decoding will not work
        vec![
            "history",
            "--kubeconfig",
            kubernetes_config.as_ref().to_str().unwrap(),
            "--namespace",
            namespace,
            "-o",
            "json",
            release_name,
        ],
        envs.clone(),
        |line| output_string = line,
        |line| {
            if line.contains("Error: release: not found") {
                info!("{}", line)
            } else {
                error!("{}", line)
            }
        },
    );

    // TODO better check, release not found

    let mut results = match serde_json::from_str::<Vec<HelmHistoryRow>>(output_string.as_str()) {
        Ok(x) => x,
        Err(_) => vec![],
    };

    // unsort results by revision number
    let _ = results.sort_by_key(|x| x.revision);
    // there is no performance penalty to do it in 2 operations instead of one, but who really cares anyway
    let _ = results.reverse();

    Ok(results)
}

pub fn helm_exec_upgrade_with_override_file<P>(
    kubernetes_config: P,
    namespace: &str,
    release_name: &str,
    chart_root_dir: P,
    override_file: &str,
    envs: Vec<(&str, &str)>,
) -> Result<(), CommandError>
where
    P: AsRef<Path>,
{
    helm_exec_with_output(
        vec![
            "upgrade",
            "--kubeconfig",
            kubernetes_config.as_ref().to_str().unwrap(),
            "--create-namespace",
            "--install",
            "--history-max",
            "50",
            "--wait",
            "--namespace",
            namespace,
            release_name,
            chart_root_dir.as_ref().to_str().unwrap(),
            "-f",
            override_file,
        ],
        envs,
        |line| info!("{}", line.as_str()),
        |line| {
            // don't crash errors if releases are not found
            if line.contains("Error: release: not found") {
                info!("{}", line)
            } else {
                error!("{}", line)
            }
        },
    )
}

pub fn helm_exec_with_upgrade_history_with_override<P>(
    kubernetes_config: P,
    namespace: &str,
    release_name: &str,
    chart_root_dir: P,
    override_file: &str,
    envs: Vec<(&str, &str)>,
) -> Result<Option<HelmHistoryRow>, CommandError>
where
    P: AsRef<Path>,
{
    // do exec helm upgrade
    info!(
        "exec helm upgrade for namespace {} and chart {}",
        namespace,
        chart_root_dir.as_ref().to_str().unwrap()
    );

    let _ = helm_exec_upgrade_with_override_file(
        kubernetes_config.as_ref(),
        namespace,
        release_name,
        chart_root_dir.as_ref(),
        override_file,
        envs.clone(),
    )?;

    // list helm history
    info!(
        "exec helm history for namespace {} and chart {}",
        namespace,
        chart_root_dir.as_ref().to_str().unwrap()
    );

    let helm_history_rows = helm_exec_history(kubernetes_config.as_ref(), namespace, release_name, &envs)?;

    // take the last deployment from helm history - or return none if there is no history
    Ok(helm_history_rows
        .first()
        .map(|helm_history_row| helm_history_row.clone()))
}

pub fn helm_get_chart_version<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
    namespace: Option<&str>,
    chart_name: String,
) -> Option<Version>
where
    P: AsRef<Path>,
{
    match helm_list(kubernetes_config, envs, namespace) {
        Ok(deployed_charts) => {
            for chart in deployed_charts {
                if chart.name == chart_name {
                    return chart.version;
                }
            }

            None
        }
        Err(_) => None,
    }
}

/// List deployed helm charts
///
/// # Arguments
///
/// * `kubernetes_config` - kubernetes config path
/// * `envs` - environment variables required for kubernetes connection
/// * `namespace` - list charts from a kubernetes namespace or use None to select all namespaces
pub fn helm_list<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
    namespace: Option<&str>,
) -> Result<Vec<HelmChart>, CommandError>
where
    P: AsRef<Path>,
{
    let mut output_vec: Vec<String> = Vec::new();
    let mut helm_args = vec![
        "list",
        "--kubeconfig",
        kubernetes_config.as_ref().to_str().unwrap(),
        "-o",
        "json",
    ];
    match namespace {
        Some(ns) => helm_args.append(&mut vec!["-n", ns]),
        None => helm_args.push("-A"),
    }

    let _ = helm_exec_with_output(helm_args, envs, |line| output_vec.push(line), |line| error!("{}", line));

    let output_string: String = output_vec.join("");
    let values = serde_json::from_str::<Vec<HelmListItem>>(output_string.as_str());
    let mut helms_charts: Vec<HelmChart> = Vec::new();

    match values {
        Ok(all_helms) => {
            for helm in all_helms {
                let raw_version = helm.chart.replace(format!("{}-", helm.name).as_str(), "");
                let version = match Version::from_str(raw_version.as_str()) {
                    Ok(v) => Some(v),
                    Err(_) => None,
                };

                helms_charts.push(HelmChart::new(helm.name, helm.namespace, version))
            }
        }
        Err(e) => {
            let message_safe = "Error while deserializing all helms names";
            return Err(CommandError::new(
                format!("{}, error: {}", message_safe, e),
                Some(message_safe.to_string()),
            ));
        }
    }

    Ok(helms_charts)
}

pub fn helm_upgrade_diff_with_chart_info<P>(
    kubernetes_config: P,
    envs: &Vec<(String, String)>,
    chart: &ChartInfo,
) -> Result<(), CommandError>
where
    P: AsRef<Path>,
{
    let mut environment_variables = envs.clone();
    environment_variables.push(("HELM_NAMESPACE".to_string(), chart.get_namespace_string()));

    let mut args_string: Vec<String> = vec![
        "diff",
        "upgrade",
        "--no-color",
        "--allow-unreleased",
        "--kubeconfig",
        kubernetes_config.as_ref().to_str().unwrap(),
    ]
    .into_iter()
    .map(|x| x.to_string())
    .collect();

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
            return Err(CommandError::new(
                format!(
                    "{}\nContent\n{}\nError: {}",
                    safe_message.to_string(),
                    value_file.yaml_content,
                    e
                ),
                Some(safe_message.to_string()),
            ));
        };

        args_string.push("-f".to_string());
        args_string.push(file_path.clone());
    }

    // add last elements
    args_string.push(chart.name.to_string());
    args_string.push(chart.path.to_string());

    helm_exec_with_output(
        args_string.iter().map(|x| x.as_str()).collect(),
        environment_variables
            .iter()
            .map(|x| (x.0.as_str(), x.1.as_str()))
            .collect(),
        |line| info!("{}", line),
        |line| error!("{}", line),
    )
}

fn helm_exec(args: Vec<&str>, envs: Vec<(&str, &str)>) -> Result<(), CommandError> {
    helm_exec_with_output(
        args,
        envs,
        |line| {
            span!(Level::INFO, "{}", "{}", line);
        },
        |line_err| {
            span!(Level::INFO, "{}", "{}", line_err);
        },
    )
}

fn helm_exec_with_output<F, X>(
    args: Vec<&str>,
    envs: Vec<(&str, &str)>,
    stdout_output: F,
    stderr_output: X,
) -> Result<(), CommandError>
where
    F: FnMut(String),
    X: FnMut(String),
{
    // Note: Helm CLI use spf13/cobra lib for the CLI; One function is mainly used to return an error if a command failed.
    // Helm returns an error each time a command does not succeed as they want. Which leads to handling error with status code 1
    // It means that the command successfully ran, but it didn't terminate as expected
    let mut cmd = QoveryCommand::new("helm", &args, &envs);
    match cmd.exec_with_timeout(Duration::max_value(), stdout_output, stderr_output) {
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

#[cfg(test)]
mod tests {
    use crate::cloud_provider::helm::{ChartInfo, ChartSetValue};
    use crate::cmd::helm::{Helm, HelmError};
    use crate::cmd::utilities::QoveryCommand;
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::Duration;

    const KUBECONFIG_PATH: &str = "/home/erebe/.kube/config";

    struct HelmTestCtx {
        helm: Helm,
        chart: ChartInfo,
    }

    impl HelmTestCtx {
        fn cleanup(&self) {
            let ret = self.helm.uninstall(&self.chart);
            assert!(ret.is_ok())
        }

        fn new(release_name: &str) -> HelmTestCtx {
            let mut chart = ChartInfo::new_from_custom_namespace(
                release_name.to_string(),
                "tests/helm/simple_nginx".to_string(),
                "default".to_string(),
                300,
                vec![],
                false,
                None,
            );
            chart.wait = true;
            chart.atomic = true;
            let helm = Helm::new(KUBECONFIG_PATH).unwrap();

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
    fn test_release_exist() {
        let HelmTestCtx { ref helm, ref chart } = HelmTestCtx::new("test-release-exist");
        let ret = helm.check_release_exist(chart);

        assert!(matches!(ret, Err(HelmError::ReleaseDoesNotExist(test)) if test == chart.name))
    }

    #[test]
    fn test_rollback() {
        let HelmTestCtx { ref helm, ref chart } = HelmTestCtx::new("test-rollback");

        // check release does not exist yet
        let ret = helm.rollback(&chart);
        assert!(matches!(ret, Err(HelmError::ReleaseDoesNotExist(test)) if test == chart.name));

        // install it
        let ret = helm.upgrade(&chart, &vec![]);
        assert!(matches!(ret, Ok(())));

        // First revision cannot be rollback
        let ret = helm.rollback(&chart);
        assert!(matches!(ret, Err(HelmError::CannotRollback(_))));

        // 2nd upgrade
        let ret = helm.upgrade(&chart, &vec![]);
        assert!(matches!(ret, Ok(())));

        // Rollback should be ok now
        let ret = helm.rollback(&chart);
        assert!(matches!(ret, Ok(())));
    }

    #[test]
    fn test_upgrade() {
        let HelmTestCtx { ref helm, ref chart } = HelmTestCtx::new("test-upgrade");

        // check release does not exist yet
        let ret = helm.check_release_exist(&chart);
        assert!(matches!(ret, Err(HelmError::ReleaseDoesNotExist(test)) if test == chart.name));

        // install it
        let ret = helm.upgrade(&chart, &vec![]);
        assert!(matches!(ret, Ok(())));

        // check now it exists
        let ret = helm.check_release_exist(&chart);
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
        let ret = helm.check_release_exist(&chart);
        assert!(matches!(ret, Err(HelmError::ReleaseDoesNotExist(test)) if test == chart.name));

        // install it
        let ret = helm.upgrade(&chart, &vec![]);
        assert!(matches!(ret, Err(HelmError::Timeout(_, _, _))));

        // Release should not exist if it fails
        let ret = helm.check_release_exist(&chart);
        assert!(matches!(ret, Err(HelmError::ReleaseDoesNotExist(test)) if test == chart.name));
    }

    #[test]
    fn test_upgrade_with_lock_during_install() {
        // We want to check that we manage to install a chart even if a lock is present while it was the first installation
        let HelmTestCtx { ref helm, ref chart } = HelmTestCtx::new("test-upgrade-with-lock-install");

        // check release does not exist yet
        let ret = helm.check_release_exist(&chart);
        assert!(matches!(ret, Err(HelmError::ReleaseDoesNotExist(test)) if test == chart.name));

        // Spawn our task killer
        let barrier = Arc::new(Barrier::new(2));
        std::thread::spawn({
            let barrier = barrier.clone();
            let chart_name = chart.name.clone();
            move || {
                barrier.wait();
                thread::sleep(Duration::from_millis(3000));
                let mut cmd = QoveryCommand::new("pkill", &vec!["-9", "-f", &format!("helm.*{}", chart_name)], &vec![]);
                let _ = cmd.exec();
            }
        });

        // install it
        barrier.wait();
        let ret = helm.upgrade(&chart, &vec![]);
        assert!(matches!(ret, Err(_)));

        // Release should be locked
        let ret = helm.check_release_exist(&chart);
        assert!(matches!(ret, Ok(release) if release.is_locked()));

        // New installation should work even if a lock is present
        let ret = helm.upgrade(&chart, &vec![]);
        assert!(matches!(ret, Ok(())));

        // Release should not be locked anymore
        let ret = helm.check_release_exist(&chart);
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
        let ret = helm.check_release_exist(&chart);
        assert!(matches!(ret, Err(HelmError::ReleaseDoesNotExist(test)) if test == chart.name));

        // First install
        let ret = helm.upgrade(&chart, &vec![]);
        assert!(matches!(ret, Ok(())));

        // Spawn our task killer
        let barrier = Arc::new(Barrier::new(2));
        std::thread::spawn({
            let barrier = barrier.clone();
            let chart_name = chart.name.clone();
            move || {
                barrier.wait();
                thread::sleep(Duration::from_millis(3000));
                let mut cmd = QoveryCommand::new("pkill", &vec!["-9", "-f", &format!("helm.*{}", chart_name)], &vec![]);
                let _ = cmd.exec();
            }
        });

        chart.values = vec![ChartSetValue {
            key: "initialDelaySeconds".to_string(),
            value: "6".to_string(),
        }];
        barrier.wait();
        let ret = helm.upgrade(&chart, &vec![]);
        assert!(matches!(ret, Err(_)));

        // Release should be locked
        let ret = helm.check_release_exist(&chart);
        assert!(matches!(ret, Ok(release) if release.is_locked() && release.version == 2));

        // New installation should work even if a lock is present
        let ret = helm.upgrade(&chart, &vec![]);
        assert!(matches!(ret, Ok(())));

        // Release should not be locked anymore
        let ret = helm.check_release_exist(&chart);
        assert!(matches!(ret, Ok(release) if !release.is_locked() && release.version == 4));
    }

    #[test]
    fn test_uninstall() {
        let HelmTestCtx { ref helm, ref chart } = HelmTestCtx::new("test-uninstall");

        // check release does not exist yet
        let ret = helm.check_release_exist(&chart);
        assert!(matches!(ret, Err(HelmError::ReleaseDoesNotExist(test)) if test == chart.name));

        // deleting something that does not exist should not be an issue
        let ret = helm.uninstall(&chart);
        assert!(matches!(ret, Ok(())));

        // install it
        let ret = helm.upgrade(&chart, &vec![]);
        assert!(matches!(ret, Ok(())));

        // check now it exists
        let ret = helm.check_release_exist(&chart);
        assert!(matches!(ret, Ok(_)));

        // Delete it
        let ret = helm.uninstall(&chart);
        assert!(matches!(ret, Ok(())));

        // check release does not exist anymore
        let ret = helm.check_release_exist(&chart);
        assert!(matches!(ret, Err(HelmError::ReleaseDoesNotExist(test)) if test == chart.name));
    }
}

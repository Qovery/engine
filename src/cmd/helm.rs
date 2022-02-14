use std::io::{Error, Write};
use std::path::Path;

use tracing::{error, info, span, Level};

use crate::cloud_provider::helm::{deploy_charts_levels, ChartInfo, CommonChart};
use crate::cloud_provider::service::ServiceType;
use crate::cmd::helm::HelmLockErrors::{IncorrectFormatDate, NotYetExpired, ParsingError};
use crate::cmd::kubectl::{kubectl_exec_delete_secret, kubectl_exec_get_secrets};
use crate::cmd::structs::{HelmChart, HelmHistoryRow, HelmListItem, Secrets};
use crate::cmd::utilities::QoveryCommand;
use crate::error::{SimpleError, SimpleErrorKind};
use crate::errors::CommandError;
use chrono::{DateTime, Duration, Utc};
use core::time;
use retry::delay::Fixed;
use retry::Error::Operation;
use retry::OperationResult;
use semver::Version;
use std::fs::File;
use std::str::FromStr;
use std::thread;

const HELM_DEFAULT_TIMEOUT_IN_SECONDS: u32 = 300;

pub enum Timeout<T> {
    Default,
    Value(T),
}

impl Timeout<u32> {
    fn value(&self) -> u32 {
        match *self {
            Timeout::Default => HELM_DEFAULT_TIMEOUT_IN_SECONDS,
            Timeout::Value(t) => t,
        }
    }
}

pub fn helm_exec_with_upgrade_history<P>(
    kubernetes_config: P,
    namespace: &str,
    release_name: &str,
    selector: Option<String>,
    chart_root_dir: P,
    timeout: Timeout<u32>,
    envs: Vec<(&str, &str)>,
    service_type: ServiceType,
) -> Result<Option<HelmHistoryRow>, CommandError>
where
    P: AsRef<Path>,
{
    // do exec helm upgrade
    info!(
        "exec helm upgrade for namespace {} and chart {}",
        &namespace,
        chart_root_dir.as_ref().to_str().unwrap()
    );

    let path = match chart_root_dir.as_ref().to_str().is_some() {
        true => chart_root_dir.as_ref().to_str().unwrap(),
        false => "",
    }
    .to_string();

    let current_chart = CommonChart {
        chart_info: ChartInfo::new_from_custom_namespace(
            release_name.to_string(),
            path.clone(),
            namespace.to_string(),
            timeout.value() as i64,
            match service_type {
                ServiceType::Database(_) => vec![format!("{}/q-values.yaml", path)],
                _ => vec![],
            },
            false,
            selector,
        ),
    };

    let environment_variables: Vec<(String, String)> =
        envs.iter().map(|x| (x.0.to_string(), x.1.to_string())).collect();

    deploy_charts_levels(
        kubernetes_config.as_ref(),
        &environment_variables,
        vec![vec![Box::new(current_chart)]],
        false,
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
                return helm_exec_uninstall(
                    kubernetes_config,
                    chart_namespace.as_str(),
                    chart_info.name.as_str(),
                    environment_variables.to_owned(),
                );
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

        if should_clean_helm_lock {
            match clean_helm_lock(
                &kubernetes_config,
                chart.get_namespace_string().as_str(),
                &chart.name,
                chart.timeout_in_seconds,
                envs.clone(),
            ) {
                Ok(_) => info!("Helm lock detected and cleaned"),
                Err(e) => warn!("Couldn't cleanup Helm lock. {:?}", e.message()),
            }
        }

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

pub fn clean_helm_lock<P>(
    kubernetes_config: P,
    namespace: &str,
    release_name: &str,
    timeout: i64,
    envs: Vec<(&str, &str)>,
) -> Result<(), CommandError>
where
    P: AsRef<Path>,
{
    let selector = format!("name={}", release_name);
    let timeout_i64 = timeout;

    let result = retry::retry(Fixed::from_millis(3000).take(5), || {
        // get secrets for this helm deployment
        let result = match kubectl_exec_get_secrets(&kubernetes_config, namespace, &selector, envs.clone()) {
            Ok(x) => x,
            Err(e) => return OperationResult::Retry(e),
        };

        // get helm release name (secret) containing the lock and clean if possible
        match helm_get_secret_lock_name(&result, timeout_i64.clone()) {
            Ok(x) => return OperationResult::Ok(x),
            Err(e) => match e.kind {
                ParsingError => OperationResult::Retry(CommandError::new(e.message, None)),
                IncorrectFormatDate => OperationResult::Retry(CommandError::new(e.message, None)),
                NotYetExpired => {
                    if e.wait_before_release_lock.is_none() {
                        return OperationResult::Retry(CommandError::new_from_safe_message(
                            "Missing helm time to wait information, before releasing the lock".to_string(),
                        ));
                    };

                    let time_to_wait = e.wait_before_release_lock.unwrap() as u64;
                    // wait 2min max to avoid the customer to re-launch a job or exit
                    if time_to_wait < 120 {
                        info!("waiting {}s before retrying the deployment...", time_to_wait);
                        thread::sleep(time::Duration::from_secs(time_to_wait));
                    } else {
                        return OperationResult::Err(CommandError::new(e.message, None));
                    }

                    // retrieve now the secret
                    match helm_get_secret_lock_name(&result, timeout_i64.clone()) {
                        Ok(x) => OperationResult::Ok(x),
                        Err(e) => OperationResult::Err(CommandError::new(e.message, None)),
                    }
                }
            },
        }
    });

    match result {
        Err(err) => {
            return match err {
                retry::Error::Operation { .. } => Err(CommandError::new_from_safe_message(format!(
                    "internal error while trying to deploy helm chart {}",
                    release_name
                ))),
                retry::Error::Internal(err) => Err(CommandError::new_from_safe_message(err)),
            }
        }
        Ok(x) => {
            if let Err(e) = kubectl_exec_delete_secret(&kubernetes_config, namespace, x.as_str(), envs.clone()) {
                return Err(e);
            };
            Ok(())
        }
    }
}

pub enum HelmDeploymentErrors {
    SimpleError,
    HelmLockError,
}

#[derive(Debug)]
pub enum HelmLockErrors {
    ParsingError,
    IncorrectFormatDate,
    NotYetExpired,
}

#[derive(Debug)]
pub struct HelmLockError {
    kind: HelmLockErrors,
    message: String,
    wait_before_release_lock: Option<i64>,
}

/// Get helm secret name containing the lock
pub fn helm_get_secret_lock_name(secrets_items: &Secrets, timeout: i64) -> Result<String, HelmLockError> {
    match secrets_items.items.last() {
        None => Err(HelmLockError {
            kind: ParsingError,
            message: "couldn't parse the list of secrets, it's certainly empty".to_string(),
            wait_before_release_lock: None,
        }),
        Some(x) => {
            let creation_time = match DateTime::parse_from_rfc3339(&x.metadata.creation_timestamp) {
                Ok(x) => x,
                Err(e) => {
                    return Err(HelmLockError {
                        kind: IncorrectFormatDate,
                        message: format!("incorrect format date input from secrets. {:?}", e),
                        wait_before_release_lock: None,
                    })
                }
            };
            let now = Utc::now().timestamp();
            let max_timeout = creation_time.timestamp() + timeout;

            // not yet expired
            if &now < &max_timeout {
                let time_to_wait = &max_timeout - &now;
                return Err(HelmLockError {
                    kind: NotYetExpired,
                    message: format!(
                        "helm lock has not yet expired, please wait {}s before retrying",
                        &time_to_wait
                    ),
                    wait_before_release_lock: Some(time_to_wait),
                });
            }

            //expired
            Ok(x.metadata.name.to_string())
        }
    }
}

pub fn helm_exec_uninstall_with_chart_info<P>(
    kubernetes_config: P,
    envs: &Vec<(&str, &str)>,
    chart: &ChartInfo,
) -> Result<(), CommandError>
where
    P: AsRef<Path>,
{
    helm_exec_with_output(
        vec![
            "uninstall",
            "--kubeconfig",
            kubernetes_config.as_ref().to_str().unwrap(),
            "--namespace",
            chart.get_namespace_string().as_str(),
            &chart.name,
        ],
        envs.clone(),
        |line| info!("{}", line.as_str()),
        |line| error!("{}", line.as_str()),
    )
}

pub fn helm_exec_uninstall<P>(
    kubernetes_config: P,
    namespace: &str,
    release_name: &str,
    envs: Vec<(&str, &str)>,
) -> Result<(), CommandError>
where
    P: AsRef<Path>,
{
    helm_exec_with_output(
        vec![
            "uninstall",
            "--kubeconfig",
            kubernetes_config.as_ref().to_str().unwrap(),
            "--namespace",
            namespace,
            release_name,
        ],
        envs,
        |line| info!("{}", line.as_str()),
        |line| error!("{}", line.as_str()),
    )
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

pub fn helm_uninstall_list<P>(
    kubernetes_config: P,
    helm_list: Vec<HelmChart>,
    envs: Vec<(&str, &str)>,
) -> Result<String, SimpleError>
where
    P: AsRef<Path>,
{
    let mut output_vec: Vec<String> = Vec::new();

    for chart in helm_list {
        match helm_exec_with_output(
            vec![
                "uninstall",
                "-n",
                chart.namespace.as_str(),
                chart.name.as_str(),
                "--kubeconfig",
                kubernetes_config.as_ref().to_str().unwrap(),
            ],
            envs.clone(),
            |line| output_vec.push(line),
            |line| error!("{}", line),
        ) {
            Ok(_) => info!(
                "Helm uninstall succeed for {} on namespace {}",
                chart.name, chart.namespace
            ),
            Err(_) => info!(
                "Helm history found for release name {} on namespace {}",
                chart.name, chart.namespace
            ),
        };
    }

    Ok(output_vec.join("\n"))
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

pub fn is_chart_deployed<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
    namespace: Option<&str>,
    chart_name: String,
) -> Result<bool, CommandError>
where
    P: AsRef<Path>,
{
    let deployed_charts = helm_list(kubernetes_config, envs, namespace)?;

    for chart in deployed_charts {
        if chart.name == chart_name {
            return Ok(true);
        }
    }

    Ok(false)
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

pub fn helm_exec(args: Vec<&str>, envs: Vec<(&str, &str)>) -> Result<(), CommandError> {
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

pub fn helm_exec_with_output<F, X>(
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

#[cfg(test)]
mod tests {
    use crate::cmd::helm::helm_get_secret_lock_name;
    use crate::cmd::structs::Secrets;
    use chrono::{DateTime, NaiveDateTime, Utc};

    #[test]
    fn test_helm_lock_get_name() {
        let json_content = r#"
{
    "apiVersion": "v1",
    "items": [
        {
            "apiVersion": "v1",
            "data": {
                "release": "coucou"
            },
            "kind": "Secret",
            "metadata": {
                "creationTimestamp": "2021-09-02T23:20:36Z",
                "labels": {
                    "modifiedAt": "1632324195",
                    "name": "cert-manager",
                    "owner": "helm",
                    "status": "superseded",
                    "version": "1"
                },
                "name": "sh.helm.release.v1.cert-manager.v1",
                "namespace": "cert-manager",
                "resourceVersion": "7287406",
                "uid": "173b76c4-4f48-4544-8928-64a9b8b376d5"
            },
            "type": "helm.sh/release.v1"
        }
    ],
    "kind": "List",
    "metadata": {
        "resourceVersion": "",
        "selfLink": ""
    }
}
        "#;
        let mut secrets = serde_json::from_str::<Secrets>(json_content).unwrap();

        // expired lock should be ok
        let res = helm_get_secret_lock_name(&secrets, 300).unwrap();
        assert_eq!(res, "sh.helm.release.v1.cert-manager.v1".to_string());

        // lock is not expired yet
        let time_in_future = NaiveDateTime::from_timestamp(Utc::now().timestamp() + 30, 0);
        let time_in_future_datetime_format: DateTime<Utc> = DateTime::from_utc(time_in_future, Utc);
        secrets.items[0].metadata.creation_timestamp = time_in_future_datetime_format.to_rfc3339();
        let res = helm_get_secret_lock_name(&secrets, 300);
        assert_eq!(
            res.unwrap_err().message,
            "helm lock has not yet expired, please wait 330s before retrying".to_string()
        )
    }
}

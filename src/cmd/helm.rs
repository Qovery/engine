use std::io::{Error, Write};
use std::path::Path;

use tracing::{error, info, span, Level};

use crate::cloud_provider::helm::{get_chart_namespace, ChartInfo};
use crate::cmd::helm::HelmLockErrors::{IncorrectFormatDate, NotYetExpired, ParsingError};
use crate::cmd::kubectl::{kubectl_exec_delete_secret, kubectl_exec_get_secrets};
use crate::cmd::structs::{Helm, HelmChart, HelmHistoryRow, Item, KubernetesList};
use crate::cmd::utilities::exec_with_envs_and_output;
use crate::error::{SimpleError, SimpleErrorKind};
use chrono::{DateTime, Duration, Utc};
use core::time;
use retry::delay::Fixed;
use retry::Error::Operation;
use retry::OperationResult;
use std::fs::File;
use std::thread;

const HELM_DEFAULT_TIMEOUT_IN_SECONDS: u32 = 300;

pub enum Timeout<T> {
    Default,
    Value(T),
}

pub fn helm_exec_with_upgrade_history<P>(
    kubernetes_config: P,
    namespace: &str,
    release_name: &str,
    chart_root_dir: P,
    timeout: Timeout<u32>,
    envs: Vec<(&str, &str)>,
) -> Result<Option<HelmHistoryRow>, SimpleError>
where
    P: AsRef<Path>,
{
    // do exec helm upgrade
    info!(
        "exec helm upgrade for namespace {} and chart {}",
        namespace,
        chart_root_dir.as_ref().to_str().unwrap()
    );

    let _ = helm_exec_upgrade(
        kubernetes_config.as_ref(),
        namespace,
        release_name,
        chart_root_dir.as_ref(),
        timeout,
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

pub fn helm_exec_upgrade_with_chart_info<P>(
    kubernetes_config: P,
    envs: &Vec<(&str, &str)>,
    chart: &ChartInfo,
) -> Result<(), SimpleError>
where
    P: AsRef<Path>,
{
    let debug = false;

    let mut args_string: Vec<String> = vec![
        "upgrade",
        "--kubeconfig",
        kubernetes_config.as_ref().to_str().unwrap(),
        "--create-namespace",
        "--install",
        "--history-max",
        "50",
        "--namespace",
        get_chart_namespace(chart.namespace).as_str(),
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
            return Err(SimpleError {
                kind: SimpleErrorKind::Other,
                message: Some(format!(
                    "error while writing yaml content to file {}\n{}\n{}",
                    &file_path, value_file.yaml_content, e
                )),
            });
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
        match helm_exec_with_output(
            args,
            envs.clone(),
            |out| match out {
                Ok(line) => {
                    info!("{}", line);
                    if debug {
                        debug!("{}", line);
                    }
                    json_output_string = line
                }
                Err(err) => error!("{}", &err),
            },
            |out| match out {
                Ok(line) => {
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
                }
                Err(err) => {
                    error_message = format!("helm chart {} failed before deployment. {:?}", chart.name, err);
                    helm_error_during_deployment.message = Some(error_message.clone());
                    error!("{}", error_message);
                }
            },
        ) {
            Ok(_) => {
                if helm_error_during_deployment.message.is_some() {
                    OperationResult::Retry(helm_error_during_deployment)
                } else {
                    OperationResult::Ok(())
                }
            }
            Err(e) => OperationResult::Retry(e),
        }
    });

    match result {
        Ok(_) => Ok(()),
        Err(Operation { error, .. }) => return Err(error),
        Err(retry::Error::Internal(e)) => return Err(SimpleError::new(SimpleErrorKind::Other, Some(e))),
    }
}

pub fn helm_exec_upgrade<P>(
    kubernetes_config: P,
    namespace: &str,
    release_name: &str,
    chart_root_dir: P,
    timeout: Timeout<u32>,
    envs: Vec<(&str, &str)>,
) -> Result<(), SimpleError>
where
    P: AsRef<Path>,
{
    let timeout_i64 = match timeout {
        Timeout::Value(v) => v + HELM_DEFAULT_TIMEOUT_IN_SECONDS,
        Timeout::Default => HELM_DEFAULT_TIMEOUT_IN_SECONDS,
    } as i64;
    let timeout_string = format!("{}s", &timeout_i64);

    let result = retry::retry(Fixed::from_millis(15000).take(3), || {
        let mut clean_lock = false;
        match helm_exec_with_output(
            vec![
                "upgrade",
                "--kubeconfig",
                kubernetes_config.as_ref().to_str().unwrap(),
                "--create-namespace",
                "--install",
                "--history-max",
                "50",
                "--timeout",
                timeout_string.as_str(),
                "--wait",
                "--namespace",
                namespace,
                release_name,
                chart_root_dir.as_ref().to_str().unwrap(),
            ],
            envs.clone(),
            |out| match out {
                Ok(line) => info!("{}", line.as_str()),
                Err(err) => error!("{}", err),
            },
            |out| match out {
                Ok(line) => {
                    error!("{}", line.as_str());
                    if line.contains("another operation (install/upgrade/rollback) is in progress") {
                        clean_lock = true;
                    }
                }
                Err(err) => error!("{}", err),
            },
        ) {
            Ok(_) => {
                if clean_lock {
                    return match clean_helm_lock(
                        &kubernetes_config,
                        &namespace,
                        &release_name,
                        timeout_i64.clone(),
                        envs.clone(),
                    ) {
                        Ok(_) => {
                            let e = SimpleError {
                                kind: SimpleErrorKind::Other,
                                message: Some("Helm lock detected and cleaned".to_string()),
                            };
                            OperationResult::Retry(e)
                        }
                        Err(e) => OperationResult::Err(e),
                    };
                };
                OperationResult::Ok(())
            }
            Err(e) => OperationResult::Retry(e),
        }
    });

    match result {
        Ok(_) => Ok(()),
        Err(Operation { error, .. }) => return Err(error),
        Err(retry::Error::Internal(e)) => return Err(SimpleError::new(SimpleErrorKind::Other, Some(e))),
    }
}

pub fn clean_helm_lock<P>(
    kubernetes_config: P,
    namespace: &str,
    release_name: &str,
    timeout: i64,
    envs: Vec<(&str, &str)>,
) -> Result<(), SimpleError>
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
                ParsingError => OperationResult::Retry(SimpleError {
                    kind: SimpleErrorKind::Other,
                    message: Some(e.message),
                }),
                IncorrectFormatDate => OperationResult::Retry(SimpleError {
                    kind: SimpleErrorKind::Other,
                    message: Some(e.message),
                }),
                NotYetExpired => {
                    if e.wait_before_release_lock.is_none() {
                        return OperationResult::Retry(SimpleError {
                            kind: SimpleErrorKind::Other,
                            message: Some(
                                "missing helm time to wait information, before releasing the lock".to_string(),
                            ),
                        });
                    };

                    let time_to_wait = e.wait_before_release_lock.unwrap() as u64;
                    // wait 2min max to avoid the customer to re-launch a job or exit
                    if time_to_wait < 120 {
                        info!("waiting {}s before retrying the deployment...", time_to_wait);
                        thread::sleep(time::Duration::from_secs(time_to_wait));
                    } else {
                        return OperationResult::Err(SimpleError {
                            kind: SimpleErrorKind::Other,
                            message: Some(e.message),
                        });
                    }

                    // retrieve now the secret
                    match helm_get_secret_lock_name(&result, timeout_i64.clone()) {
                        Ok(x) => OperationResult::Ok(x),
                        Err(e) => OperationResult::Err(SimpleError {
                            kind: SimpleErrorKind::Other,
                            message: Some(e.message),
                        }),
                    }
                }
            },
        }
    });

    match result {
        Err(err) => {
            return match err {
                retry::Error::Operation { .. } => Err(SimpleError {
                    kind: SimpleErrorKind::Other,
                    message: Some(format!(
                        "internal error while trying to deploy helm chart {}",
                        release_name
                    )),
                }),
                retry::Error::Internal(err) => Err(SimpleError::new(SimpleErrorKind::Other, Some(err))),
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
pub fn helm_get_secret_lock_name(secrets_items: &KubernetesList<Item>, timeout: i64) -> Result<String, HelmLockError> {
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
) -> Result<(), SimpleError>
where
    P: AsRef<Path>,
{
    helm_exec_with_output(
        vec![
            "uninstall",
            "--kubeconfig",
            kubernetes_config.as_ref().to_str().unwrap(),
            "--namespace",
            get_chart_namespace(chart.namespace).as_str(),
            &chart.name,
        ],
        envs.clone(),
        |out| match out {
            Ok(line) => info!("{}", line.as_str()),
            Err(err) => error!("{}", err),
        },
        |out| match out {
            Ok(line) => error!("{}", line.as_str()),
            Err(err) => error!("{}", err),
        },
    )
}

pub fn helm_exec_uninstall<P>(
    kubernetes_config: P,
    namespace: &str,
    release_name: &str,
    envs: Vec<(&str, &str)>,
) -> Result<(), SimpleError>
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
        |out| match out {
            Ok(line) => info!("{}", line.as_str()),
            Err(err) => error!("{}", err),
        },
        |out| match out {
            Ok(line) => error!("{}", line.as_str()),
            Err(err) => error!("{}", err),
        },
    )
}

pub fn helm_exec_history<P>(
    kubernetes_config: P,
    namespace: &str,
    release_name: &str,
    envs: &Vec<(&str, &str)>,
) -> Result<Vec<HelmHistoryRow>, SimpleError>
where
    P: AsRef<Path>,
{
    let mut output_string = String::new();
    match helm_exec_with_output(
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
        |out| match out {
            Ok(line) => output_string = line,
            Err(err) => error!("{:?}", err),
        },
        |out| match out {
            Ok(line) => {
                if line.contains("Error: release: not found") {
                    info!("{}", line)
                } else {
                    error!("{}", line)
                }
            }
            Err(err) => error!("{:?}", err),
        },
    ) {
        Ok(_) => info!("Helm history success for release name: {}", release_name),
        Err(_) => info!("Helm history found for release name: {}", release_name),
    };
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
            |out| match out {
                Ok(line) => output_vec.push(line),
                Err(err) => error!("{:?}", err),
            },
            |out| match out {
                Ok(line) => error!("{}", line),
                Err(err) => error!("{:?}", err),
            },
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
) -> Result<(), SimpleError>
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
        |out| match out {
            Ok(line) => info!("{}", line.as_str()),
            Err(err) => error!("{}", err),
        },
        |out| match out {
            // don't crash errors if releases are not found
            Ok(line) if line.contains("Error: release: not found") => info!("{}", line.as_str()),
            Ok(line) => error!("{}", line.as_str()),
            Err(err) => error!("{}", err),
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
) -> Result<Option<HelmHistoryRow>, SimpleError>
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
) -> Result<Vec<HelmChart>, SimpleError>
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

    let _ = helm_exec_with_output(
        helm_args,
        envs,
        |out| match out {
            Ok(line) => output_vec.push(line),
            Err(err) => error!("{}", err),
        },
        |out| match out {
            Ok(line) => error!("{}", line.as_str()),
            Err(err) => error!("{}", err),
        },
    );

    let output_string: String = output_vec.join("");
    let values = serde_json::from_str::<Vec<Helm>>(output_string.as_str());
    let mut helms_charts: Vec<HelmChart> = Vec::new();

    match values {
        Ok(all_helms) => {
            for helm in all_helms {
                helms_charts.push(HelmChart::new(helm.name, helm.namespace))
            }
        }
        Err(e) => {
            let message = format!("Error while deserializing all helms names {}", e);
            error!("{}", message.as_str());
            return Err(SimpleError::new(SimpleErrorKind::Other, Some(message)));
        }
    }

    Ok(helms_charts)
}

pub fn helm_upgrade_diff_with_chart_info<P>(
    kubernetes_config: P,
    envs: &Vec<(String, String)>,
    chart: &ChartInfo,
) -> Result<(), SimpleError>
where
    P: AsRef<Path>,
{
    let mut environment_variables = envs.clone();
    environment_variables.push(("HELM_NAMESPACE".to_string(), get_chart_namespace(chart.namespace)));

    let mut args_string: Vec<String> = vec![
        "diff",
        "upgrade",
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
            return Err(SimpleError {
                kind: SimpleErrorKind::Other,
                message: Some(format!(
                    "error while writing yaml content to file {}\n{}\n{}",
                    &file_path, value_file.yaml_content, e
                )),
            });
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
        |out| match out {
            Ok(line) => info!("{}", line),
            Err(err) => error!("{}", &err),
        },
        |out| match out {
            Ok(line) => error!("{}", line),
            Err(err) => error!("{}", err),
        },
    )
}

pub fn helm_exec(args: Vec<&str>, envs: Vec<(&str, &str)>) -> Result<(), SimpleError> {
    helm_exec_with_output(
        args,
        envs,
        |line| {
            span!(Level::INFO, "{}", "{}", line.unwrap());
        },
        |line_err| {
            span!(Level::INFO, "{}", "{}", line_err.unwrap());
        },
    )
}

pub fn helm_exec_with_output<F, X>(
    args: Vec<&str>,
    envs: Vec<(&str, &str)>,
    stdout_output: F,
    stderr_output: X,
) -> Result<(), SimpleError>
where
    F: FnMut(Result<String, Error>),
    X: FnMut(Result<String, Error>),
{
    // Note: Helm CLI use spf13/cobra lib for the CLI; One function is mainly used to return an error if a command failed.
    // Helm returns an error each time a command does not succeed as they want. Which leads to handling error with status code 1
    // It means that the command successfully ran, but it didn't terminate as expected
    match exec_with_envs_and_output("helm", args, envs, stdout_output, stderr_output, Duration::max_value()) {
        Err(err) => match err.kind {
            SimpleErrorKind::Command(exit_status) => match exit_status.code() {
                Some(exit_status_code) => {
                    if exit_status_code == 1 {
                        Ok(())
                    } else {
                        Err(err)
                    }
                }
                None => Err(err),
            },
            SimpleErrorKind::Other => Err(err),
        },
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use crate::cmd::helm::helm_get_secret_lock_name;
    use crate::cmd::structs::{Item, KubernetesList};
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
                "release": "SDRzSUFBQUFBQUFDLzZSWWFYT2pPTGYrS3k3dTE4UU5PSFRIcm5vL0dEb0dPVFlaT3pFQ1RVOTFTWUlBdGdRMG00UDc5bisvSmNCYmIzUG5uVXFsclBWc2VwNmpJNzVLQ2VhQk5KRm9tZ2QrVWtnM1VweThwdExrcS9RYTUwWDUyUTh5bGphQkwwMGtWVmFWVy9uOXJYcjNJbzhubWp5UlB3enZGZlc5ZkQrKy80Q2tHNG5oZjdqQkQxaFF0a3ZiVGtIek9DdmpOSkVtRWtpS0VqTTJvQ25QeENMcFJpcEtYRmFGTkpHS0tndnlJdkFEWC9wMkk5RUk1NlV3bUFjbDluR0pSZnZhcTF1YUpxOXhLTjFJZFpBWG5RSjVxQXpsSDlST0IxYkErS0NWT1hoTjg4RmpSWUk4Q2NwQWhBWm5zWE1TVUt2dFNPWmNpcFJ1cExMSmhHcWNaU3ltdUpYNzdVWmlLZDFKazZSaTdFWXFBNTR4TEVSTy9qeVplaHA4OXprS21IQndXR1pNR05pNkpBV2pvbjRNczFsZzZSSGhxOUEzSXdaTUxTSndFeExWNi90MjZybHplUkdtWHhZalpXd2s1WWZGczc1REVHV0ViMExBN1pweVowZVNaZW1wNHdweGxpeDRLeU1HaGpKK1hXVWZna1lPa2Vsd3ozVUszNWhXS3pXS2FMS3FYdmlzUk0vVHl1R3p3b2ZPb2R2bi9PRnpwNkdjN2RDemZnK3NkZU5EYlFzK2VnZGc3VVBmbW1mRXNSWEVVUllZMDNqeERNSlhWMzRNUmtVSlRLZENoajUrWGFXUHZXOGZLWGNpMzl5RVhtZHo1TU0zR1poSUllYWJCcXlaNHNHM0RISEdrS0ZIMUpxR3BMWHA3dEZSTjBLWFFyamQ3WGRYb2IxZGhsNXIrekwwdUxQMVhPZUFudlVEVVJVR0hrckY0MDVEdUNNamQ5bkwwVG1HVG9Hc1plaTVjd2JNdDR4QUppTW9ZbjB2NHB2UlpwcDZ5YTZMOWNQNmFkUG9sUWVWakhBYTBwSE92R1pYR1R2R2dUVm54SFFpcW02T3NyZEUxV1FQc29vMitoYWJzOFkzMnIwTW1Fd0dscDhSY3g5NmZCUDZydDM1MStoUjUvdis1T2RqUEw0Nkl3dzFCb3o1bG96bURKbmFZUUh0bW5DVW9VYmpQbndydW4wZ1hMZ25MR1Fvdmp6RE5yYXRIUzhqeEdneXo1QzVPWitSb1VFUHZpbklYVllvY1FwaS91c3pMMFJNenZMWFhRdytwbU5ncmhuaU00VllxM0N4NjNHODYvQzlZQ2dpbHNOb28vM2FWcE54WU5vMVNkWVJodHJoSk52UW5oRjhZNTVyczZPOEMvOStuRE4wbVNaTzVUVlRkZGwwL21Hb3ZQZ1FjZXlHSVlqbEdGaW5lREppMmV5aXY2ZWNWYjdwaFNCMkRvdG41d0RpYWJYaFRvSGc3SUNldGFmT3B0NjJMbDRLNGNzV3I2MXV6c3JOeU9HSXN6dGdnQkxFK3ZuTW9iYTcwczFYL3o4K2RSdytZckhMSCtJTVZWYVR1TVdhUWxWbkI4eTVCcXgxS3ZEYVk3VHc0SndSNHg5aHI5OTdoYnVmeHVYNzNITHNPMmZiN2dXWHFQa1dlZW9tQkhFUkEyUCtDdUovbjJ1SXFwUkV2UXVKT1lzUmZEc1ljVnFmMXdzc3NnbzlUMk5QSFRjSXJpdmFLSUxESEVOYUhmZUFlRnFLUGRoMEN2S3NIYkF4N256L21JYkJxQWd4MUxiRWNuYS9rdE9mU3d5TXU2T3RmN3VIcWdKTHRreEc4MlY3TnRieWNuL0hBMFA3MkoyQnBsTkxoOGlkSHpBY1Z5MTIrR3hQRFMzMzRaeFJyakhmRkhtQTFZdVdVM1pHMUx2M3dDby8vRnJHUHFTdVUvdFgzRHZHTnhONXVjSWpKMGJ1dkVMdVduQTJJODI0OUtBV0lkWFpMZUJjZTRyMUQwRnp4WXNYNU01VkRLKzRkTUozMjIvU3g4M0pkL0JmbmR2UFk2Zjh6dTZXcDUyOWVrYTRYZmh3L1N2Y2Q1eU9wNytQczhvcU9scEhoTnZzSjNIbzhzTnZZckFhelpubnJsblAwLzRPR2ZPK2Z6akYwWnh0UFhXc2tLUy92MXliL2NOWU5UNW5XN1E1eWxrLzlmNlYzMkh0ZEVmOHVHZFZlU2Q3citxUjdtNDNvcitMNmZVOUZ1OHU3NEtUcnl0b2I4bklxWHlqUDROZjUrY0wzZk56Ky9KT1RHeEdFNVI1cXFON3FsMzdVSlA3R3VsbmVML095K0YvL2lOOXUvbEpUZGNWb0J4bnc0WmZGSFZZWlJYNm1JYXJ2aGg3Z2JPOUFBNTJMc25vdlJsY1liNDUyM251T25vSzA3QXYvTjZMQzY4djZCNUJYMURRa1M2UzVYdGdpaUpuVTlJUk8vaW1VeHJ4OUVTYVZvWXhEUkVzRHdzK0t6MDNxd21maldpamJZa3ExNTQ2THFrNXJoRFU1S2RZUHgyTWtBRU1QWCt5bHFYbjZ2dW5XRzhCdmhEZzJXZG4reDVhdXppR2J3TGs5OTIrYWZXMDNSeUFWVHdLM2QyL0xpN3ltcmIybjhaU0JHZUZiNGFYWTFkRUFxWmQrSzR0SXhkVXhCeHZQYmdQTWJ3clBiamUwVmlMYUNLS0o3YTNqKzBybmRNUVdIb3RDajFSS0NCb0s1UnZ2cHQzOW5TMGJoQ2NsYjN0Unp1NEI5OEszNHdhTW5JU2JPZ1ppWlVJbWV0bXdXY05OUjlDN0U3VnZuMXAvL2hTRHJYbU5ZR09qRTFIb2MzMC9kUExnN0xjcDJjOUpxcHA0a2VVcjhKRlBLMlJ1OTR1Um5ORzFYSGh4MXBMMGt0NW5qcmJZbk1UTGcvVFM1MEZVY2Y3SzczY0tZZzYyMTJPRVhNY0lYTWVFWE5XZVdvZkIwdnVmMXZTTkI3VWt1dmlYNGtJbnlVSXJsOEZMbHJ3ZDlob0U1cm9mMy9XeUowM1pEUS9uSFNiZHVTcEVRTWZsMmNici94dWsrNFhvcklLR0NBRThYUjNRZjdUaGVLcTY0b3E0d2E1ZGswc3dadGxTOVR2L1RpVFZwRDByeHVweHF3U2I3Q3ZFc2NKRGdQL3MzZ0FULzc4NitaeTRITWVGQ2tUNzBZeDllMUdLbWdVY0h4OHk3M0c3UG9kTjR3Q3h1TXdTZlBnekhQUTZDdlBYY3ZpY3FHTkxwTkd6NUNxMVZRVS9HckVTS3pIUG1RRk1sbUZHbjN2UVR2M29NOW9jL2NJR24yRFRYWUFscTFRUzY5cHNoYnRGTUczQXBoK1FWUVFFamlUUFZVa1UxcDBqNCtaakYzRWdLVkh2aGxlejV1ekNrM1RMVEExaHRTWjNGN3FSclJ2K2VwRzR2RXh3dTQ2QmNaRHRvajFQd2gvMDRESUJjLzYvdWdEc0hSR1k3MFFSZUFpVEt2MXpIN2RqTloxeTZOR1B4Vlh6czUrQWVZNkUveGVjRC96amZ2dTEyUUo0ZU1HcmRMS1M3Sm1zZTkrTWZRcmtXdU1XRXRSSTlaR3ljWFl3ZWQzdFJFdlJiNHNpYkRibkVlZVdpclUwTnRjUS9mcGwwVmlqK2cwL2JMZzh3aTMvWFhaOWNjTmh2VHhNV2xqQ2oxM25wR1Jjd0FQN0dIdExoOFhpZDRRTldQZWFQVzRFSThiK0ZBYmNWcjVwcktuZlB6RmlEV1ZxbmFOekUzZEpYcUJodjdMd2c4Z2t0S1VENHRJK2cyWUpHWFkva2sza2pLVWgvSlFrWVJFanBQNE5TaEthU0xkM3Q1K1N2NW44SnhXT1EwbWcrdXZHZTkrY2J0OFNuWng0azhHUmp1NHhObW41UHpkWWpLb2xVL0o4VXZKNUZNeUdBamNubVFmQjRvTUM0MjdpZ1MzUlZPVUFSY3pESk9BRmUydXdTRFlGVVBNOFNGTjhMNFkwcFMvb3luUDBpUkl5aXR4ZzhIdXZyakZXZGFMYTRkUDJvMDBEd1NISm9QLzdSWVBKOXBvOExWcnQycnlQTTJMY3o4S01DdWpjMzkzK2tBem9Ld3F5aUFmc3BSaU5vaVRXK3o3K1JEbkdSN0UyZnV1Y1NGNU1NaFN2eGpFU1JIUUtnOHVKNnFzS1BNQTg4dXhWOHhZR2VWcEZVWS9sMzFlL08zY3pQS1VCMlVVVk1WZ01sYTAwWG5tTmMzM09QY0h3OEc3b0tUdk9tUU14VkdlMTFCTW8yQXdrczhqTEUyemN5OFBXSXI5eTFuc0U4eHdRbnQzZWtzNk1GNUY5anF1M3lzNkc5ZURkTkJEOUNUMjhtT2FjaU9kUUNOTnBBdlVTTi8rTHdBQS8vOFpyL0YvWWhRQUFBPT0="
            },
            "kind": "Secret",
            "metadata": {
                "creationTimestamp": "2021-06-24T09:50:08Z",
                "labels": {
                    "modifiedAt": "1624529703",
                    "name": "coredns",
                    "owner": "helm",
                    "status": "superseded",
                    "version": "1"
                },
                "name": "sh.helm.release.v1.coredns.v1",
                "namespace": "kube-system",
                "resourceVersion": "562542",
                "selfLink": "/api/v1/namespaces/kube-system/secrets/sh.helm.release.v1.coredns.v1",
                "uid": "a635e9ea-f793-4369-918a-7d644e85988b"
            },
            "type": "helm.sh/release.v1"
        },
        {
            "apiVersion": "v1",
            "data": {
                "release": "SDRzSUFBQUFBQUFDLzZSWWEzT2JPcmYrS3g3TzE4VGxFdExZTSs4SFEyT1FZNU50SjBhZzNUMGRTUkRBbG9CeXNZTjcrdC9QQ0lndmJkTjk5cnNuazdHdTY2Ym5XVnJpbTVSaUhrcGppV1pGR0tTbGRDVWw2VXNtamI5SkwwbFJWbCtDTUdkWkV3YlNXRkpsVmJtV2I2L1ZtMmQ1Tk5ibHNmeHhlS2VvdC9MZDZPNGprcTRraHQvZG9LaGo3WGFzNk1NYjVWYlhkVVcvRXh1Q2tJVlZ1N1R0bExSSThpckpVbWtzcmZPb3dFRTRvQm5QeFNMcFNpb3JYTldsTkphT0tyNWZTVFRHUlNYTTVXR0ZBMXhoMGI3MDZacG02VXNTU1ZmU0xpektUcnc4VklieVQwb25BenRrZk5ES0hMeGt4ZUNoSm1HUmhsVW9Bb1B6eEQwSzJLbnRTTzZlaTVTdXBLckpoV3FjNXl5aHVKWDcvVXBpR2QxSzQ3Um03RXFxUXA0ekxFU08venlhZWh6ODhDVU9XUjRXNWJES21UQ3dkVWtLdFhMM0VPWFQwRFppd3BkUllNVU1XSHBNNERvaXF0LzNuY3ozWnZJOHlyN09OV1ZrcHRYSCtaT3hSUkRsaEs4andKMGQ1ZTZXcEl2S1YwYzE0aXlkODFaR0FreGw5TExNUDRhTkhDSEw1YjdubG9FNXFaZHFITk4wV1QvemFZV2VKclhMcDJVQTNVTzN6LzBqNEc1RE9kdWlKK01PMktzbWdQb0dmUElQd041SGdUM0xpZXNvaUtNOE5DZkovQWxFTDU3OEVHcGxCU3kzUnFZeGVsbG1ENzF2bnloMzQ4QmFSMzVuY3h6QVZ4bFlTQ0hXcXc3c3FlTEQxeHh4eHBCcHhOU2VSS1MxNmViQlZkZENsMEs0MCszM2xwR3pXVVIrYS9zaThybTc4VDMzZ0o2TUExRVZCdTRyeGVkdVE3Z3JJMi9SeXpFNGhtNko3RVhrZXpNR3JOZWNRQ1lqS0dKOUorS2IwMmFTK2VtMmkvWDk2bkhkR0xVUGxaeHdHbEhOWUg2enJjMHQ0OENlTVdLNU1WWFhiN0kzUk5WbEg3S2FOc1lHVzlNbU1OdTlERmhNQm5hUUUyc2YrWHdkQlo3VCtkY1ljZWY3L3VqblF6SzZPQ01NZFFiTTJZWm9NNFlzL1RDSHpvNXdsS05HNXdGOExidDlJSnA3Unl6a0tEay93emEyclIzUEdtSTBuZVhJV3AvT3lOU2hEMThWNUMxcWxMb2xzZjcxbVpjaUppZjVxeTRHbjdJUnNGWU04YWxDN0dVMDMvWTQzbmI0bmpNVUU5dGx0TkhmdDlWaUhGak9qcVNyR0VQOWNKUnQ2azhJdmpMZmM5aWJ2RFAvZnA0ekRabW1idTAzRTNYUmRQNWhxRHdIRUhIc1JSRkk1QVRZeDNneVlqdnNyTCtubk5XQjVVY2djUS96Si9jQWtrbTk1bTZKNFBTQW52VEh6cWJldGk1ZUN1R0xGcSt0YnM2cXRlWnl4TmtOTUVFRkV1TjA1bERmWHVqbXkvOGZuem9PdjJHeHl4L2lERlcySTBtTE5ZV3E3aFpZTXgzWXEwemd0Y2RvNmNNWkkrWS93bDYvOXdKM3Y0ekxqN25scmUrZWJMc1RYS0xXYSt5cjZ3Z2taUUxNMlF0SS9uMnVJYXBTRWZVbUl0WTBRZkQxWUNiWjdyUmVZSkhWNkdtUytPcW9RWEJWMDBZUkhPWVkwdnB0RDBnbWxkaURMYmNrVC9vQm02UE85MDlaRkdwbGhLRytJYmE3ZlU5T2Z5NEpNRy9lYlAzYlBWUVZXSEprb3MwVzdkbllpL1A5SFE5TS9WTjNCcnBCYlFNaWIzYkFjRlMzMk9IVFBUWDFJb0F6UnJuT0FrdmtBYmFidDV4eWNxTGUzQUs3K3ZpK2pIMUVQWGNYWEhEdkxiNjV5TXMxMXR3RWViTWFlU3ZCMlp3MG84cUhlb3hVZHp1SE0vMHhNVDZHelFVdm5wRTNVekc4NE5JUjMyMi95UjdXUjkvQmYzVnV2NDZkOGp1N1c1NTI5aG81NFU0WndOVjd1Tzg0blV4K0gyZVYxVlJieFlRNzdCZHg2UExEYjJLdzFHYk05MWFzNTJsL2g0eDQzejhjNDJoTk43NDZVa2phMzErZXcvNWhySnFBc3cxYXY4bFpQZmIrVlQ5ZzdYaEgvTHhuV2Z0SGV5L3FrZTV1TitPL2krbmxQWlpzeisrQ282OUw2R3lJNXRhQjJaL0IrL241VFBmczFENi9FMU9IMFJUbHZ1b2F2dXJzQXFqTGZZMzBLN3hmNXVYb1AvK1J2bC85b3FickNsQ084MkhEejRvNnJMSWFmY3FpWlYrTVBjUHBYZ0FIdStkazlGOU5yckRBbW01OWJ4VS9SbG5VRjM2MzRzTHJDN29IMEJjVVZETkVzcndGbGloeTFoWFYyQ0d3M01wTUprZlN0RExNU1lSZ2RaanphZVY3K1k3d3FVWWJmVU5VZWVlcm80cGFveHBCWFg1TWpPUEJDQm5BTklwSGUxSDVuckYvVEl3VzRITUJubjErc3UrK3RZdGorQ3BBZnRmdG05U1BtL1VCMk9XRDBOMzlHK0lpMzlIVy91TllodUMwREt6b2ZPeUNTTUJ5eXNCelpPU0JtbGlqalEvM0VZWTNsUTlYVzVyb01VMUY4Y1Qyemx2N1F1Y2tBcmF4RTRXZUtCUVFkQlRLMXovTXUzdXFyUm9FcDFWdis1c2QzSWV2WldERkRkSGNGSnRHVGhJbFJ0YXFtZk5wUTYzN0NIc1R0VytmMno4NmwwUHQyWTVBVjhhV3E5Qm1jdnY0Zks4czl0bEpqNFYyTkExaXlwZlJQSm5za0xmYXpMVVpvK3FvREJLOUplbTVQRitkYnJDMWpoYUh5Ym5Pa3Fpai9ZVmU3cFpFblc3UHg0ZzFpcEUxaTRrMXJYMjFqNE10OTc4dGFSb2Y2dWxsOGEvRWhFOVRCRmN2QWhjdCtEdHN0QWxOOUg4OGErVE5HcUxORGtmZGxoUDdhc3pBcDhYSnhndS8yNlQ3bGFpc0JpYUlRRExabnBIL2VLRjQ2cXFteXFoQm5yTWp0dUROb2lYcWozNmNTQ3RJK3RlVnRNT3NGbSt3YnhMSEtZN0M0SXQ0L283Ly9PdnFmT0JMRVpZWkUrOUdNZlg5U2lwcEhITDg5cFo3U2RqbE8yNFloNHduVVpvVjRZbm5vREdXdnJlU3hlVkNHME1talpFalZkOVJVZkNyTVNPSmtRU1FsY2hpTldxTXZRK2R3b2NCbzgzTkEyaU1OYmJZQWRpT1FtMWpSOU9WYUdjSXZwYkFDa3FpZ29qQXFleXJJcG5Tc250OFRHWHNJUVpzSXc2czZITGVtdFpva20yQXBUT2tUdVgyVWpmamZjdFhMeGFQRHcxN3F3eVk5L2s4TWY0Zy9GVUhJaGM4R2ZzM0g0QnRNSm9ZcFNnQzUxRldyNmJPeTFwYjdWb2VOY2F4dUhLM3pqT3dWcm5nOTV3SGVXRGVkYjhXU3drZk5XaVoxWDZhTi9OOTk0dGhVSXRjWXlaNmhocXhOazdQeGc0QnY5bVp5VUxreTRvSXU2MVo3S3VWUWsyanpUVjBuMzJkcDQ1R0o5blhPWi9GdU8ydnFxNC9hakNrRHc5cEcxUG9lN09jYU80QjNMUDdsYmQ0bUtkR1E5U2MrZHJ5WVM0ZU4vQitaeVpaSFZqS252TFJWelBSVmFvNk8yU3RkMTJpRjJqb3Z5ejhCQ0lweS9pd2pLWGZnRWxTaHUyZmRDVXBRM2tvRHhWSlNPUTRUVjdDc3BMRzB2WDE5ZWYwZndaUFdWM1FjRHk0L0pyeDRaM2I1WE82VGRKZ1BERGJ3UVhPUDZlbjd4Ymp3VTc1bkw1OUtSbC9UZ2NEZ2R1ajdMZUJNc2RDNDdZbTRYWFpsRlhJeFF6REpHUmx1MnN3Q0xmbEVITjh5Rks4TDRjMDR4OW94dk1zRGRQcVF0eGdzTDBycjNHZTkrTGE0YU4yTXl0Q3dhSHg0SCs3eGNPeHJnMitkZTFXVFZGa1JYbnF4eUZtVlh6cWI0OGZhQWFVMVdVVkZrT1dVY3dHU1hxTmc2QVk0aUxIZ3lTLzdScG5rZ2VEUEF2S1FaS1dJYTJMOEh5aXpzdXFDREUvSDN2QmpGVnhrZFZSL0d2WnA4WGZUODI4eUhoWXhXRmREc1lqUmRkT015OVpzY2RGTUJnT1BvUVYvZEFoWXlpTzhyU0dZaHFIQTAwK2piQXN5MCs5SW1RWkRzNW5jVUF3d3ludDNla3Q2Y0I0RWRuTHVQNm82R1JjRDlKQkQ5R2oyUE9QYWJkWDBoRTAwbGc2UTQzMC9mOENBQUQvL3pheFIzbGdGQUFB"
            },
            "kind": "Secret",
            "metadata": {
                "creationTimestamp": "2021-06-24T12:36:16Z",
                "labels": {
                    "modifiedAt": "1624538178",
                    "name": "coredns",
                    "owner": "helm",
                    "status": "deployed",
                    "version": "2"
                },
                "name": "sh.helm.release.v1.coredns.v2",
                "namespace": "kube-system",
                "resourceVersion": "603757",
                "selfLink": "/api/v1/namespaces/kube-system/secrets/sh.helm.release.v1.coredns.v6",
                "uid": "b71dfb0d-8c4e-4592-8155-322bde8d402f"
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
        let mut secrets = serde_json::from_str::<KubernetesList<Item>>(json_content).unwrap();

        // expired lock should be ok
        let res = helm_get_secret_lock_name(&secrets, 300).unwrap();
        assert_eq!(res, "sh.helm.release.v1.coredns.v2".to_string());

        // lock is not expired yet
        let time_in_future = NaiveDateTime::from_timestamp(Utc::now().timestamp() + 30, 0);
        let time_in_future_datetime_format: DateTime<Utc> = DateTime::from_utc(time_in_future, Utc);
        secrets.items[1].metadata.creation_timestamp = time_in_future_datetime_format.to_rfc3339();
        let res = helm_get_secret_lock_name(&secrets, 300);
        assert_eq!(
            res.unwrap_err().message,
            "helm lock has not yet expired, please wait 330s before retrying".to_string()
        )
    }
}

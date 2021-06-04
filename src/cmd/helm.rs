use std::io::Error;
use std::path::Path;

use tracing::{error, info, span, Level};

use crate::cloud_provider::helm::{get_chart_namespace, ChartInfo};
use crate::cmd::structs::{Helm, HelmChart, HelmHistoryRow};
use crate::cmd::utilities::exec_with_envs_and_output;
use crate::error::{SimpleError, SimpleErrorKind};
use chrono::Duration;

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

    let helm_history_rows = helm_exec_history(kubernetes_config.as_ref(), namespace, release_name, envs)?;

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
    let mut args_string: Vec<String> = vec![
        "upgrade",
        "-o",
        "json",
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

    // add last elements
    args_string.push(chart.name.to_string());
    args_string.push(chart.path.to_string());

    let args = args_string.iter().map(|x| x.as_str()).collect();

    let mut json_output_string = String::new();
    let mut error_message = String::new();
    let mut helm_error_during_deployment = SimpleError {
        kind: SimpleErrorKind::Other,
        message: None,
    };
    match helm_exec_with_output(
        args,
        envs.clone(),
        |out| match out {
            Ok(line) => json_output_string = line,
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
                return Err(helm_error_during_deployment);
            }
        }
        Err(e) => {
            return Err(SimpleError {
                kind: SimpleErrorKind::Other,
                message: Some(format!("{}. {:?}", error_message, e.message)),
            })
        }
    }

    Ok(())
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
    let timeout = format!(
        "{}s",
        match timeout {
            Timeout::Value(v) => v + HELM_DEFAULT_TIMEOUT_IN_SECONDS,
            Timeout::Default => HELM_DEFAULT_TIMEOUT_IN_SECONDS,
        }
    );

    helm_exec_with_output(
        vec![
            "upgrade",
            "--kubeconfig",
            kubernetes_config.as_ref().to_str().unwrap(),
            "--create-namespace",
            "--install",
            "--history-max",
            "50",
            "--timeout",
            timeout.as_str(),
            "--wait",
            "--namespace",
            namespace,
            release_name,
            chart_root_dir.as_ref().to_str().unwrap(),
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
    envs: Vec<(&str, &str)>,
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
        envs,
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

    let helm_history_rows = helm_exec_history(kubernetes_config.as_ref(), namespace, release_name, envs)?;

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

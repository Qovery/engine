use std::ffi::OsStr;
use std::fmt::{Display, Formatter};
use std::io::Error;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};

use dirs::home_dir;
use retry::delay::Fibonacci;
use retry::OperationResult;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cmd::structs::{Helm, HelmHistoryRow};
use crate::cmd::utilities::exec_with_envs_and_output;
use crate::constants::{KUBECONFIG, TF_PLUGIN_CACHE_DIR};
use crate::error::{SimpleError, SimpleErrorKind};

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

    let helm_history_rows =
        helm_exec_history(kubernetes_config.as_ref(), namespace, release_name, envs)?;

    // take the last deployment from helm history - or return none if there is no history
    Ok(match helm_history_rows.first() {
        Some(helm_history_row) => Some(helm_history_row.clone()),
        None => None,
    })
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
            Ok(line) => error!("{}", line),
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
    helmlist: Vec<String>,
    envs: Vec<(&str, &str)>,
) -> Result<String, SimpleError>
where
    P: AsRef<Path>,
{
    let mut output_vec: Vec<String> = Vec::new();
    let helmlist_string = helmlist.join(" ");
    match helm_exec_with_output(
        vec![
            "uninstall",
            helmlist_string.as_str(),
            "--kubeconfig",
            kubernetes_config.as_ref().to_str().unwrap(),
        ],
        envs,
        |out| match out {
            Ok(line) => output_vec.push(line),
            Err(err) => error!("{:?}", err),
        },
        |out| match out {
            Ok(line) => error!("{}", line),
            Err(err) => error!("{:?}", err),
        },
    ) {
        Ok(_) => info!("Helm uninstall fail with : {}", helmlist_string.clone()),
        Err(_) => info!(
            "Helm history found for release name: {}",
            helmlist_string.clone()
        ),
    };
    Ok(output_vec.join(""))
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

    let helm_history_rows =
        helm_exec_history(kubernetes_config.as_ref(), namespace, release_name, envs)?;

    // take the last deployment from helm history - or return none if there is no history
    Ok(match helm_history_rows.first() {
        Some(helm_history_row) => Some(helm_history_row.clone()),
        None => None,
    })
}

pub fn helm_list<P>(
    kubernetes_config: P,
    envs: Vec<(&str, &str)>,
) -> Result<Vec<String>, SimpleError>
where
    P: AsRef<Path>,
{
    let mut output_vec: Vec<String> = Vec::new();
    helm_exec_with_output(
        vec![
            "list",
            "-A",
            "--kubeconfig",
            kubernetes_config.as_ref().to_str().unwrap(),
            "-o",
            "json",
        ],
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
    let mut helms_name: Vec<String> = Vec::new();
    match values {
        Ok(all_helms) => {
            for helm in all_helms {
                helms_name.push(helm.name)
            }
        }
        Err(e) => {
            let message = format!("Error while deserializing all helms names {}", e);
            error!("{}", message.as_str());
            return Err(SimpleError::new(SimpleErrorKind::Other, Some(message)));
        }
    }
    Ok(helms_name)
}

pub fn helm_exec(args: Vec<&str>, envs: Vec<(&str, &str)>) -> Result<(), SimpleError> {
    helm_exec_with_output(
        args,
        envs,
        |line| {
            info!("{}", line.unwrap());
        },
        |line| {
            error!("{}", line.unwrap());
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
    match exec_with_envs_and_output("helm", args, envs, stdout_output, stderr_output) {
        Err(err) => return Err(err),
        _ => {}
    };

    Ok(())
}

pub fn kubectl_exec_with_output<F, X>(
    args: Vec<&str>,
    envs: Vec<(&str, &str)>,
    stdout_output: F,
    stderr_output: X,
) -> Result<(), SimpleError>
where
    F: FnMut(Result<String, Error>),
    X: FnMut(Result<String, Error>),
{
    match exec_with_envs_and_output("kubectl", args, envs, stdout_output, stderr_output) {
        Err(err) => return Err(err),
        _ => {}
    };

    Ok(())
}

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

use crate::constants::{KUBECONFIG, TF_PLUGIN_CACHE_DIR};

fn command<P>(binary: P, args: Vec<&str>, envs: Option<Vec<(&str, &str)>>) -> Command
where
    P: AsRef<Path>,
{
    let s_binary = binary
        .as_ref()
        .to_str()
        .unwrap()
        .split_whitespace()
        .map(|x| x.to_string())
        .collect::<Vec<_>>();

    let (current_dir, _binary) = if s_binary.len() == 1 {
        (None, s_binary.first().unwrap().clone())
    } else {
        (
            Some(s_binary.first().unwrap().clone()),
            s_binary.get(1).unwrap().clone(),
        )
    };

    let mut cmd = Command::new(&_binary);

    cmd.args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if current_dir.is_some() {
        cmd.current_dir(current_dir.unwrap());
    }

    if envs.is_some() {
        envs.unwrap().into_iter().for_each(|(k, v)| {
            cmd.env(k, v);
        });
    }

    cmd
}

pub fn exec<P>(binary: P, args: Vec<&str>) -> Result<(), CmdError>
where
    P: AsRef<Path>,
{
    let command_string = command_to_string(binary.as_ref(), &args);
    info!("command: {}", command_string.as_str());

    let exit_status = match command(binary, args, None).spawn().unwrap().wait() {
        Ok(x) => x,
        Err(err) => return Err(CmdError::Io(err)),
    };

    if exit_status.success() {
        return Ok(());
    }

    Err(CmdError::Exec(exit_status))
}

pub fn exec_with_envs<P>(
    binary: P,
    args: Vec<&str>,
    envs: Vec<(&str, &str)>,
) -> Result<(), CmdError>
where
    P: AsRef<Path>,
{
    let command_string = command_with_envs_to_string(binary.as_ref(), &args, &envs);
    info!("command: {}", command_string.as_str());

    let exit_status = match command(binary, args, Some(envs)).spawn().unwrap().wait() {
        Ok(x) => x,
        Err(err) => return Err(CmdError::Io(err)),
    };

    if exit_status.success() {
        return Ok(());
    }

    Err(CmdError::Exec(exit_status))
}

fn _with_output<F, X>(mut child: Child, mut stdout_output: F, mut stderr_output: X) -> Child
where
    F: FnMut(Result<String, Error>),
    X: FnMut(Result<String, Error>),
{
    let stdout_reader = BufReader::new(child.stdout.as_mut().unwrap());
    for line in stdout_reader.lines() {
        stdout_output(line);
    }

    let stderr_reader = BufReader::new(child.stderr.as_mut().unwrap());
    for line in stderr_reader.lines() {
        stderr_output(line);
    }

    child
}

pub fn exec_with_output<P, F, X>(
    binary: P,
    args: Vec<&str>,
    stdout_output: F,
    stderr_output: X,
) -> Result<(), CmdError>
where
    P: AsRef<Path>,
    F: FnMut(Result<String, Error>),
    X: FnMut(Result<String, Error>),
{
    let command_string = command_to_string(binary.as_ref(), &args);
    info!("command: {}", command_string.as_str());

    let mut child = _with_output(
        command(binary, args, None).spawn().unwrap(),
        stdout_output,
        stderr_output,
    );

    let exit_status = match child.wait() {
        Ok(x) => x,
        Err(err) => return Err(CmdError::Io(err)),
    };

    if exit_status.success() {
        return Ok(());
    }

    Err(CmdError::Exec(exit_status))
}

pub fn exec_with_envs_and_output<P, F, X>(
    binary: P,
    args: Vec<&str>,
    envs: Vec<(&str, &str)>,
    stdout_output: F,
    stderr_output: X,
) -> Result<(), CmdError>
where
    P: AsRef<Path>,
    F: FnMut(Result<String, Error>),
    X: FnMut(Result<String, Error>),
{
    let command_string = command_with_envs_to_string(binary.as_ref(), &args, &envs);
    info!("command: {}", command_string.as_str());

    let mut child = _with_output(
        command(binary, args, Some(envs)).spawn().unwrap(),
        stdout_output,
        stderr_output,
    );

    let exit_status = match child.wait() {
        Ok(x) => x,
        Err(err) => return Err(CmdError::Io(err)),
    };

    if exit_status.success() {
        return Ok(());
    }

    Err(CmdError::Exec(exit_status))
}

fn terraform_exec_with_init_validate_plan(
    root_dir: &str,
    first_time_init_terraform: bool,
) -> Result<(), CmdError> {
    // terraform init
    let init_args = if first_time_init_terraform {
        vec!["init"]
    } else {
        vec!["init"]
    };

    //TODO print
    terraform_exec(root_dir, init_args)?;

    // terraform validate config
    terraform_exec(root_dir, vec!["validate"])?;

    // terraform plan
    terraform_exec(root_dir, vec!["plan", "-out", "tf_plan"])?;

    Ok(())
}

pub fn terraform_exec_with_init_validate_plan_apply(
    root_dir: &str,
    first_time_init_terraform: bool,
) -> Result<(), CmdError> {
    // terraform init and plan
    terraform_exec_with_init_validate_plan(root_dir, first_time_init_terraform);

    // terraform apply
    terraform_exec(root_dir, vec!["apply", "-auto-approve", "tf_plan"])?;

    Ok(())
}

pub fn terraform_exec_with_init_validate_plan_destroy(root_dir: &str) -> Result<(), CmdError> {
    // terraform init and plan
    terraform_exec_with_init_validate_plan(root_dir, false);

    // terraform destroy
    terraform_exec(root_dir, vec!["destroy", "-auto-approve"])?;

    Ok(())
}

pub fn terraform_exec(root_dir: &str, args: Vec<&str>) -> Result<(), CmdError> {
    let home_dir = home_dir().expect("Could not find $HOME");
    let tf_plugin_cache_dir = format!("{}/.terraform.d/plugin-cache", home_dir.to_str().unwrap());

    match exec_with_envs_and_output(
        format!("{} terraform", root_dir).as_str(),
        args,
        vec![(TF_PLUGIN_CACHE_DIR, tf_plugin_cache_dir.as_str())],
        |line: Result<String, std::io::Error>| {
            info!("{}", line.unwrap());
        },
        |line: Result<String, std::io::Error>| {
            error!("{}", line.unwrap());
        },
    ) {
        Err(err) => return Err(err),
        _ => {}
    };

    Ok(())
}

pub fn helm_exec_with_upgrade_history<P>(
    kubernetes_config: P,
    namespace: &str,
    release_name: &str,
    chart_root_dir: P,
    envs: Vec<(&str, &str)>,
) -> Result<Option<HelmHistoryRow>, CmdError>
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
    envs: Vec<(&str, &str)>,
) -> Result<(), CmdError>
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
) -> Result<(), CmdError>
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
) -> Result<Vec<HelmHistoryRow>, CmdError>
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

pub fn helm_exec(args: Vec<&str>, envs: Vec<(&str, &str)>) -> Result<(), CmdError> {
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
) -> Result<(), CmdError>
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
) -> Result<(), CmdError>
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
// return the output of "binary_name" --version
pub fn run_version_command_for(binary_name: &str) -> String {
    let mut output_from_cmd = String::new();
    exec_with_output(
        binary_name,
        vec!["--version"],
        |r_out| match r_out {
            Ok(s) => output_from_cmd.push_str(&s.to_owned()),
            Err(e) => error!("Error while getting stdout from {} {}", binary_name, e),
        },
        |r_err| match r_err {
            Ok(s) => error!("Error executing {}", binary_name),
            Err(e) => error!("Error while getting stderr from {} {}", binary_name, e),
        },
    );
    output_from_cmd
}

pub fn does_binary_exist<S>(binary: S) -> bool
where
    S: AsRef<OsStr>,
{
    match Command::new(binary)
        .stdout(Stdio::null())
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(_) => true,
        _ => false,
    }
}

pub fn kubectl_exec_get_external_ingress_hostname<P>(
    kubernetes_config: P,
    namespace: &str,
    selector: &str,
    envs: Vec<(&str, &str)>,
) -> Result<Option<String>, CmdError>
where
    P: AsRef<Path>,
{
    let mut _envs = Vec::with_capacity(envs.len() + 1);
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs);

    let mut output_vec: Vec<String> = Vec::with_capacity(20);
    let _ = kubectl_exec_with_output(
        vec![
            "get", "svc", "-o", "json", "-n", namespace, "-l", // selector
            selector,
        ],
        _envs,
        |out| match out {
            Ok(line) => output_vec.push(line),
            Err(err) => error!("{:?}", err),
        },
        |out| match out {
            Ok(line) => error!("{}", line),
            Err(err) => error!("{:?}", err),
        },
    )?;

    let output_string: String = output_vec.join("");

    let result =
        match serde_json::from_str::<KubernetesList<KubernetesService>>(output_string.as_str()) {
            Ok(x) => x,
            Err(err) => {
                error!("{:?}", err);
                error!("{}", output_string.as_str());
                return Err(CmdError::Io(Error::new(
                    std::io::ErrorKind::InvalidData,
                    output_string,
                )));
            }
        };

    if result.items.is_empty()
        || result
            .items
            .first()
            .unwrap()
            .status
            .load_balancer
            .ingress
            .is_empty()
    {
        return Ok(None);
    }

    // FIXME unsafe unwrap here?
    Ok(Some(
        result
            .items
            .first()
            .unwrap()
            .status
            .load_balancer
            .ingress
            .first()
            .unwrap()
            .hostname
            .clone(),
    ))
}

pub fn kubectl_exec_is_application_ready_with_retry<P>(
    kubernetes_config: P,
    namespace: &str,
    selector: &str,
    envs: Vec<(&str, &str)>,
) -> Result<Option<bool>, CmdError>
where
    P: AsRef<Path>,
{
    // TODO check this
    let result = retry::retry(Fibonacci::from_millis(3000).take(10), || {
        let r = crate::cmd::kubectl_exec_is_application_ready(
            kubernetes_config.as_ref(),
            namespace,
            selector,
            envs.clone(),
        );

        match r {
            Ok(is_ready) => match is_ready {
                Some(true) => OperationResult::Ok(true),
                _ => {
                    let t = format!("application with selector: {} is not ready yet", selector);
                    info!("{}", t.as_str());
                    OperationResult::Retry(t)
                }
            },
            Err(err) => OperationResult::Err(format!("command error: {:?}", err)),
        }
    });

    match result {
        Err(err) => match err {
            retry::Error::Operation {
                error: _,
                total_delay: _,
                tries: _,
            } => Ok(Some(false)),
            retry::Error::Internal(err) => Err(CmdError::Unexpected(err)),
        },
        Ok(_) => Ok(Some(true)),
    }
}

pub fn kubectl_exec_is_application_ready<P>(
    kubernetes_config: P,
    namespace: &str,
    selector: &str,
    envs: Vec<(&str, &str)>,
) -> Result<Option<bool>, CmdError>
where
    P: AsRef<Path>,
{
    let mut _envs = Vec::with_capacity(envs.len() + 1);
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs);

    let mut output_vec: Vec<String> = Vec::with_capacity(20);
    let _ = kubectl_exec_with_output(
        vec!["get", "pod", "-o", "json", "-n", namespace, "-l", selector],
        _envs,
        |out| match out {
            Ok(line) => output_vec.push(line),
            Err(err) => error!("{:?}", err),
        },
        |out| match out {
            Ok(line) => error!("{}", line),
            Err(err) => error!("{:?}", err),
        },
    )?;

    let output_string: String = output_vec.join("");

    let result = match serde_json::from_str::<KubernetesList<KubernetesPod>>(output_string.as_str())
    {
        Ok(x) => x,
        Err(err) => {
            error!("{:?}", err);
            error!("{}", output_string.as_str());
            return Err(CmdError::Io(Error::new(
                std::io::ErrorKind::InvalidData,
                output_string,
            )));
        }
    };

    if result.items.is_empty()
        || result
            .items
            .first()
            .unwrap()
            .status
            .container_statuses
            .is_empty()
    {
        return Ok(None);
    }

    let first_item = result.items.first().unwrap();
    let container_statuses = &first_item.status.container_statuses;

    let is_ready = container_statuses.iter().find(|cs| !cs.ready).is_none();

    Ok(Some(is_ready))
}

pub fn kubectl_exec_create_namespace<P>(
    kubernetes_config: P,
    namespace: &str,
    envs: Vec<(&str, &str)>,
) -> Result<(), CmdError>
where
    P: AsRef<Path>,
{
    let mut _envs = Vec::with_capacity(envs.len() + 1);
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs);

    let _ = kubectl_exec_with_output(
        vec!["create", "namespace", namespace],
        _envs,
        |out| match out {
            Ok(line) => info!("{}", line),
            Err(err) => error!("{:?}", err),
        },
        |out| match out {
            Ok(line) => error!("{}", line),
            Err(err) => error!("{:?}", err),
        },
    )?;

    Ok(())
}

pub fn is_contains_terraform_tfstate<P>(
    kubernetes_config: P,
    namespace: &str,
    envs: &Vec<(&str, &str)>,
) -> Result<bool, CmdError>
where
    P: AsRef<Path>,
{
    let mut _envs = Vec::with_capacity(envs.len() + 1);
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs);
    let mut exist = true;
    let _ = kubectl_exec_with_output(
        vec![
            "describe",
            "secrets/tfstate-default-state",
            "--namespace",
            namespace,
        ],
        _envs,
        |out| match out {
            Ok(line) => exist = true,
            Err(err) => error!("{:?}", err),
        },
        |out| match out {
            Ok(line) => {}
            Err(err) => error!("{:?}", err),
        },
    )?;
    Ok(exist)
}

pub fn kubectl_exec_delete_namespace<P>(
    kubernetes_config: P,
    namespace: &str,
    envs: Vec<(&str, &str)>,
) -> Result<(), CmdError>
where
    P: AsRef<Path>,
{
    match is_contains_terraform_tfstate(&kubernetes_config, &namespace, &envs) {
        Ok(exist) => match exist {
            true => {
                return Err(CmdError::Io(Error::new(
                    std::io::ErrorKind::Other,
                    "Namespace contains terraform tfstates in secret, can't delete it !",
                )));
            }
            false => info!(
                "Namespace {} doesn't contain any tfstates, able to delete it",
                namespace
            ),
        },
        Err(e) => warn!(
            "Unable to execute describe on secrets: {}. it may not exist anymore?",
            e
        ),
    };

    let mut _envs = Vec::with_capacity(envs.len() + 1);
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs);

    let _ = kubectl_exec_with_output(
        vec!["delete", "namespace", namespace],
        _envs,
        |out| match out {
            Ok(line) => info!("{}", line),
            Err(err) => error!("{:?}", err),
        },
        |out| match out {
            Ok(line) => error!("{}", line),
            Err(err) => error!("{:?}", err),
        },
    )?;

    Ok(())
}

pub fn kubectl_exec_delete_secret<P>(
    kubernetes_config: P,
    secret: &str,
    envs: Vec<(&str, &str)>,
) -> Result<(), CmdError>
where
    P: AsRef<Path>,
{
    let mut _envs = Vec::with_capacity(envs.len() + 1);
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs);

    let _ = kubectl_exec_with_output(
        vec!["delete", "secret", secret],
        _envs,
        |out| match out {
            Ok(line) => info!("{}", line),
            Err(err) => error!("{:?}", err),
        },
        |out| match out {
            Ok(line) => error!("{}", line),
            Err(err) => error!("{:?}", err),
        },
    )?;

    Ok(())
}

pub fn kubectl_exec_logs<P>(
    kubernetes_config: P,
    namespace: &str,
    selector: &str,
    envs: Vec<(&str, &str)>,
) -> Result<String, CmdError>
where
    P: AsRef<Path>,
{
    let mut _envs = Vec::with_capacity(envs.len() + 1);
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs);

    let mut output_vec: Vec<String> = Vec::with_capacity(50);
    let _ = kubectl_exec_with_output(
        vec!["logs", "--tail", "1000", "-n", namespace, "-l", selector],
        _envs,
        |out| match out {
            Ok(line) => output_vec.push(line),
            Err(err) => error!("{:?}", err),
        },
        |out| match out {
            Ok(line) => error!("{}", line),
            Err(err) => error!("{:?}", err),
        },
    )?;

    Ok(output_vec.join("\n"))
}

pub fn kubectl_exec_describe<P>(
    kubernetes_config: P,
    namespace: &str,
    selector: &str,
    envs: Vec<(&str, &str)>,
) -> Result<String, CmdError>
where
    P: AsRef<Path>,
{
    let mut _envs = Vec::with_capacity(envs.len() + 1);
    _envs.push((KUBECONFIG, kubernetes_config.as_ref().to_str().unwrap()));
    _envs.extend(envs);

    let mut output_vec: Vec<String> = Vec::with_capacity(50);
    let _ = kubectl_exec_with_output(
        vec!["describe", "pod", "-n", namespace, "-l", selector],
        _envs,
        |out| match out {
            Ok(line) => output_vec.push(line),
            Err(err) => error!("{:?}", err),
        },
        |out| match out {
            Ok(line) => error!("{}", line),
            Err(err) => error!("{:?}", err),
        },
    )?;

    Ok(output_vec.join("\n"))
}

fn command_to_string<P>(binary: P, args: &Vec<&str>) -> String
where
    P: AsRef<Path>,
{
    format!("{} {}", binary.as_ref().to_str().unwrap(), args.join(" "))
}

fn command_with_envs_to_string<P>(binary: P, args: &Vec<&str>, envs: &Vec<(&str, &str)>) -> String
where
    P: AsRef<Path>,
{
    let _envs = envs
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>();

    format!(
        "{} {} {}",
        _envs.join(" "),
        binary.as_ref().to_str().unwrap(),
        args.join(" ")
    )
}

#[derive(Debug)]
pub enum CmdError {
    Exec(ExitStatus),
    Io(Error),
    Unexpected(String),
}

impl Display for CmdError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            CmdError::Exec(status) => format!("CmdError: Exec({})", status),
            CmdError::Io(io) => format!("CmdError: IO: {}", io),
            CmdError::Unexpected(s) => format!("CmdError: Unexpected: {}", s),
        };
        write!(f, "{}", s)
    }
}
impl std::error::Error for CmdError {}

impl From<std::io::Error> for CmdError {
    fn from(err: Error) -> Self {
        CmdError::Io(err)
    }
}
impl From<CmdError> for std::io::Error {
    fn from(e: CmdError) -> Self {
        std::io::Error::new(std::io::ErrorKind::Other, e)
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct HelmHistoryRow {
    pub revision: u16,
    pub status: String,
    pub chart: String,
    pub app_version: String,
}

impl HelmHistoryRow {
    pub fn is_successfully_deployed(&self) -> bool {
        self.status == "deployed"
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
struct KubernetesList<T> {
    pub items: Vec<T>,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
struct KubernetesService {
    pub status: KubernetesServiceStatus,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
struct KubernetesServiceStatus {
    pub load_balancer: KubernetesServiceStatusLoadBalancer,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
struct KubernetesServiceStatusLoadBalancer {
    pub ingress: Vec<KubernetesServiceStatusLoadBalancerIngress>,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
struct KubernetesServiceStatusLoadBalancerIngress {
    pub hostname: String,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
struct KubernetesPod {
    pub status: KubernetesPodStatus,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
struct KubernetesPodStatus {
    pub container_statuses: Vec<KubernetesPodContainerStatus>,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
struct KubernetesPodContainerStatus {
    pub ready: bool,
}

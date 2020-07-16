use serde::{Deserialize, Serialize};
use std::borrow::Borrow;
use std::io::Error;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};

use dirs::home_dir;

use crate::constants::TF_PLUGIN_CACHE_DIR;
use std::ffi::OsStr;

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
    let exit_status = match command(binary, args, Some(envs)).spawn().unwrap().wait() {
        Ok(x) => x,
        Err(err) => return Err(CmdError::Io(err)),
    };

    if exit_status.success() {
        return Ok(());
    }

    Err(CmdError::Exec(exit_status))
}

fn _with_output<F>(mut child: Child, mut output: F) -> Child
where
    F: FnMut(Result<String, Error>),
{
    let stdout_reader = BufReader::new(child.stdout.as_mut().unwrap());
    let stderr_reader = BufReader::new(child.stderr.as_mut().unwrap());

    for line in stdout_reader.lines() {
        output(line);
    }

    for line in stderr_reader.lines() {
        output(line);
    }

    child
}

pub fn exec_with_output<P, F>(binary: P, args: Vec<&str>, mut output: F) -> Result<(), CmdError>
where
    P: AsRef<Path>,
    F: FnMut(Result<String, Error>),
{
    let mut child = _with_output(command(binary, args, None).spawn().unwrap(), output);

    let exit_status = match child.wait() {
        Ok(x) => x,
        Err(err) => return Err(CmdError::Io(err)),
    };

    if exit_status.success() {
        return Ok(());
    }

    Err(CmdError::Exec(exit_status))
}

pub fn exec_with_envs_and_output<P, F>(
    binary: P,
    args: Vec<&str>,
    envs: Vec<(&str, &str)>,
    mut output: F,
) -> Result<(), CmdError>
where
    P: AsRef<Path>,
    F: FnMut(Result<String, Error>),
{
    let mut child = _with_output(command(binary, args, Some(envs)).spawn().unwrap(), output);

    let exit_status = match child.wait() {
        Ok(x) => x,
        Err(err) => return Err(CmdError::Io(err)),
    };

    if exit_status.success() {
        return Ok(());
    }

    Err(CmdError::Exec(exit_status))
}

pub fn terraform_exec_with_init_validate_plan_apply(
    root_dir: &str,
    first_time_init_terraform: bool,
) -> Result<(), CmdError> {
    // terraform init
    let init_args = if first_time_init_terraform {
        info!("exec: terraform init -backend-config=backend.tf -no-color");
        vec!["init", "-backend-config=backend.tf", "-no-color"]
    } else {
        info!("exec: terraform init -no-color");
        vec!["init", "-no-color"]
    };

    terraform_exec(root_dir, init_args)?;

    // terraform validate config
    info!("exec: terraform validate");
    terraform_exec(root_dir, vec!["validate"])?;

    // terraform plan
    info!("exec: terraform plan -out tf_plan -no-color");
    terraform_exec(root_dir, vec!["plan", "-out", "tf_plan", "-no-color"])?;

    // terraform apply
    terraform_exec(
        root_dir,
        vec!["apply", "-auto-approve", "-no-color", "tf_plan"],
    )?;

    Ok(())
}

pub fn terraform_exec(root_dir: &str, args: Vec<&str>) -> Result<(), CmdError> {
    let home_dir = home_dir().unwrap();
    let tf_plugin_cache_dir = format!("{}/.terraform.d/plugin-cache", home_dir.to_str().unwrap());

    match exec_with_envs_and_output(
        format!("{} terraform", root_dir).as_str(),
        args,
        vec![(TF_PLUGIN_CACHE_DIR, tf_plugin_cache_dir.as_str())],
        |line| {
            info!("{}", line.unwrap());
        },
    ) {
        Err(err) => return Err(err),
        _ => {}
    };

    Ok(())
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
    helm_exec(
        vec![
            "upgrade",
            "--kubeconfig",
            kubernetes_config.as_ref().to_str().unwrap(),
            "--create-namespace",
            "--install",
            "--history-max",
            "50",
            "--wait",
            "-n",
            namespace,
            release_name,
            chart_root_dir.as_ref().to_str().unwrap(),
        ],
        envs,
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
    let mut output_json_string = String::new();
    let _ = helm_exec_with_output(
        vec![
            "history",
            "--kubeconfig",
            kubernetes_config.as_ref().to_str().unwrap(),
            "-n",
            namespace,
            "-o",
            "json",
            release_name,
        ],
        envs,
        |out| match out {
            Ok(line) => output_json_string = line,
            _ => {}
        },
    )?;

    let mut results = match serde_json::from_str::<Vec<HelmHistoryRow>>(output_json_string.as_str())
    {
        Ok(x) => x,
        Err(err) => {
            error!("{}", err.to_string());
            return Err(CmdError::Io(Error::new(
                std::io::ErrorKind::InvalidData,
                err.to_string(),
            )));
        }
    };

    // unsort results by revision number
    let _ = results.sort_by_key(|x| x.revision);
    // there is no performance penalty to do it in 2 operations instead of one, but who really cares anyway
    let _ = results.reverse();

    Ok(results)
}

pub fn helm_exec(args: Vec<&str>, envs: Vec<(&str, &str)>) -> Result<(), CmdError> {
    helm_exec_with_output(args, envs, |line| {
        info!("{}", line.unwrap());
    })
}

pub fn helm_exec_with_output<F>(
    args: Vec<&str>,
    envs: Vec<(&str, &str)>,
    mut output: F,
) -> Result<(), CmdError>
where
    F: FnMut(Result<String, Error>),
{
    match exec_with_envs_and_output("helm", args, envs, output) {
        Err(err) => return Err(err),
        _ => {}
    };

    Ok(())
}

pub fn does_binary_exist<S>(binary: S) -> bool
where
    S: AsRef<OsStr>,
{
    match Command::new(binary).spawn() {
        Ok(_) => true,
        _ => false,
    }
}

#[derive(Debug)]
pub enum CmdError {
    Exec(ExitStatus),
    Io(Error),
}

impl From<std::io::Error> for CmdError {
    fn from(err: Error) -> Self {
        CmdError::Io(err)
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

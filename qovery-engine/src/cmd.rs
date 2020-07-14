use dirs::home_dir;
use std::borrow::Borrow;
use std::io::Error;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};

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

fn _with_output<F>(mut child: Child, output: F) -> Child
where
    F: Fn(Result<String, Error>),
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

pub fn exec_with_output<P, F>(binary: P, args: Vec<&str>, output: F) -> Result<(), CmdError>
where
    P: AsRef<Path>,
    F: Fn(Result<String, Error>),
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
    output: F,
) -> Result<(), CmdError>
where
    P: AsRef<Path>,
    F: Fn(Result<String, Error>),
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

pub fn terraform_exec(root_dir: &str, args: Vec<&str>) -> Result<(), CmdError> {
    let home_dir = home_dir().unwrap();
    let tf_plugin_cache_dir = format!("{}/.terraform.d/plugin-cache", home_dir.to_str().unwrap());

    match exec_with_envs_and_output(
        format!("{} terraform", root_dir).as_str(),
        args,
        vec![("TF_PLUGIN_CACHE_DIR", tf_plugin_cache_dir.as_str())],
        |line| {
            info!("{}", line.unwrap());
        },
    ) {
        Err(err) => return Err(err),
        _ => {}
    };

    Ok(())
}

pub fn helm_exec(root_dir: &str, args: Vec<&str>, envs: Vec<(&str, &str)>) -> Result<(), CmdError> {
    match exec_with_envs_and_output(format!("{} helm", root_dir).as_str(), args, envs, |line| {
        info!("{}", line.unwrap());
    }) {
        Err(err) => return Err(err),
        _ => {}
    };

    Ok(())
}

#[derive(Debug)]
pub enum CmdError {
    Exec(ExitStatus),
    Io(Error),
}

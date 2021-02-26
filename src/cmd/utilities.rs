use std::ffi::OsStr;
use std::io::Error;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};

use crate::error::{SimpleError, SimpleErrorKind};

fn command<P>(binary: P, args: Vec<&str>, envs: Option<Vec<(&str, &str)>>, use_output: bool) -> Command
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

    if use_output {
        cmd.args(&args).stdout(Stdio::piped()).stderr(Stdio::piped());
    } else {
        cmd.args(&args).stdout(Stdio::null()).stderr(Stdio::null());
    }

    if let Some(current_dir) = current_dir {
        cmd.current_dir(current_dir);
    }

    if let Some(envs) = envs {
        envs.into_iter().for_each(|(k, v)| {
            cmd.env(k, v);
        });
    }

    cmd
}

pub fn exec<P>(binary: P, args: Vec<&str>) -> Result<(), SimpleError>
where
    P: AsRef<Path>,
{
    let command_string = command_to_string(binary.as_ref(), &args);
    info!("command: {}", command_string.as_str());

    let exit_status = match command(binary, args, None, false).spawn().unwrap().wait() {
        Ok(x) => x,
        Err(err) => return Err(SimpleError::from(err)),
    };

    if exit_status.success() {
        return Ok(());
    }

    Err(SimpleError::new(
        SimpleErrorKind::Command(exit_status),
        Some("error while executing an internal command"),
    ))
}

pub fn exec_with_envs<P>(binary: P, args: Vec<&str>, envs: Vec<(&str, &str)>) -> Result<(), SimpleError>
where
    P: AsRef<Path>,
{
    let command_string = command_with_envs_to_string(binary.as_ref(), &args, &envs);
    info!("command: {}", command_string.as_str());

    let exit_status = match command(binary, args, Some(envs), false).spawn().unwrap().wait() {
        Ok(x) => x,
        Err(err) => return Err(SimpleError::from(err)),
    };

    if exit_status.success() {
        return Ok(());
    }

    Err(SimpleError::new(
        SimpleErrorKind::Command(exit_status),
        Some("error while executing an internal command"),
    ))
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
) -> Result<(), SimpleError>
where
    P: AsRef<Path>,
    F: FnMut(Result<String, Error>),
    X: FnMut(Result<String, Error>),
{
    let command_string = command_to_string(binary.as_ref(), &args);
    info!("command: {}", command_string.as_str());

    let mut child = _with_output(
        command(binary, args, None, true).spawn().unwrap(),
        stdout_output,
        stderr_output,
    );

    let exit_status = match child.wait() {
        Ok(x) => x,
        Err(err) => return Err(SimpleError::from(err)),
    };

    if exit_status.success() {
        return Ok(());
    }

    Err(SimpleError::new(
        SimpleErrorKind::Command(exit_status),
        Some("error while executing an internal command"),
    ))
}

pub fn exec_with_envs_and_output<P, F, X>(
    binary: P,
    args: Vec<&str>,
    envs: Vec<(&str, &str)>,
    stdout_output: F,
    stderr_output: X,
) -> Result<(), SimpleError>
where
    P: AsRef<Path>,
    F: FnMut(Result<String, Error>),
    X: FnMut(Result<String, Error>),
{
    let command_string = command_with_envs_to_string(binary.as_ref(), &args, &envs);
    info!("command: {}", command_string.as_str());

    let mut child = _with_output(
        command(binary, args, Some(envs), true).spawn().unwrap(),
        stdout_output,
        stderr_output,
    );

    let exit_status = match child.wait() {
        Ok(x) => x,
        Err(err) => return Err(SimpleError::from(err)),
    };

    if exit_status.success() {
        return Ok(());
    }

    Err(SimpleError::new(
        SimpleErrorKind::Command(exit_status),
        Some("error while executing an internal command"),
    ))
}

// return the output of "binary_name" --version
pub fn run_version_command_for(binary_name: &str) -> String {
    let mut output_from_cmd = String::new();
    let _ = exec_with_output(
        binary_name,
        vec!["--version"],
        |r_out| match r_out {
            Ok(s) => output_from_cmd.push_str(&s.to_owned()),
            Err(e) => error!("Error while getting stdout from {} {}", binary_name, e),
        },
        |r_err| match r_err {
            Ok(_) => error!("Error executing {}", binary_name),
            Err(e) => error!("Error while getting stderr from {} {}", binary_name, e),
        },
    );

    output_from_cmd
}

pub fn does_binary_exist<S>(binary: S) -> bool
where
    S: AsRef<OsStr>,
{
    Command::new(binary)
        .stdout(Stdio::null())
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|mut child| child.wait())
        .is_ok()
}

pub fn command_to_string<P>(binary: P, args: &Vec<&str>) -> String
where
    P: AsRef<Path>,
{
    format!("{} {}", binary.as_ref().to_str().unwrap(), args.join(" "))
}

pub fn command_with_envs_to_string<P>(binary: P, args: &Vec<&str>, envs: &Vec<(&str, &str)>) -> String
where
    P: AsRef<Path>,
{
    let _envs = envs.iter().map(|(k, v)| format!("{}={}", k, v)).collect::<Vec<_>>();

    format!(
        "{} {} {}",
        _envs.join(" "),
        binary.as_ref().to_str().unwrap(),
        args.join(" ")
    )
}

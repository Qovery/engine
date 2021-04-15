use std::ffi::OsStr;
use std::io::{BufRead, BufReader};
use std::io::{Error, ErrorKind};
use std::path::Path;
use std::process::{Child, Command, Stdio};

use crate::cmd::utilities::CommandOutputType::{STDERR, STDOUT};
use crate::error::SimpleErrorKind::Other;
use crate::error::{SimpleError, SimpleErrorKind};
use chrono::Duration;
use itertools::Itertools;
use std::time::Instant;
use timeout_readwrite::TimeoutReader;

enum CommandOutputType {
    STDOUT(Result<String, std::io::Error>),
    STDERR(Result<String, std::io::Error>),
}

fn command<P>(binary: P, args: Vec<&str>, envs: &Vec<(&str, &str)>, use_output: bool) -> Command
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

    envs.into_iter().for_each(|(k, v)| {
        cmd.env(k, v);
    });

    cmd
}

pub fn exec<P>(binary: P, args: Vec<&str>, envs: &Vec<(&str, &str)>) -> Result<(), SimpleError>
where
    P: AsRef<Path>,
{
    let command_string = command_to_string(binary.as_ref(), &args, &envs);

    info!("command: {}", command_string.as_str());

    let exit_status = match command(binary, args, envs, false).spawn().unwrap().wait() {
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
    envs: &Vec<(&str, &str)>,
    mut stdout_output: F,
    mut stderr_output: X,
    timeout: Duration,
) -> Result<Vec<String>, SimpleError>
where
    P: AsRef<Path>,
    F: FnMut(Result<String, Error>),
    X: FnMut(Result<String, Error>),
{
    assert!(timeout.num_seconds() > 0, "Timeout cannot be a 0 or negative duration");

    let command_string = command_to_string(binary.as_ref(), &args, &envs);
    info!(
        "command with {}m timeout: {}",
        timeout.num_minutes(),
        command_string.as_str()
    );

    // Start the process
    let mut child_process = command(binary, args, &envs, true).spawn().unwrap();
    let process_start_time = Instant::now();

    // Read stdout/stderr until timeout is reached
    let reader_timeout = std::time::Duration::from_secs(10.min(timeout.num_seconds() as u64));
    let stdout_reader = BufReader::new(TimeoutReader::new(child_process.stdout.take().unwrap(), reader_timeout))
        .lines()
        .map(STDOUT);

    let stderr_reader = BufReader::new(TimeoutReader::new(
        child_process.stderr.take().unwrap(),
        std::time::Duration::from_secs(0), // don't block on stderr
    ))
    .lines()
    .map(STDERR);
    let mut command_output = Vec::new();

    for line in stdout_reader.interleave(stderr_reader) {
        match line {
            STDOUT(Err(ref err)) | STDERR(Err(ref err)) if err.kind() == ErrorKind::TimedOut => {}
            STDOUT(line) => {
                match &line {
                    Ok(x) => command_output.push(x.to_string()),
                    _ => {}
                }
                stdout_output(line)
            }
            STDERR(line) => {
                match &line {
                    Ok(x) => command_output.push(x.to_string()),
                    _ => {}
                }
                stderr_output(line)
            }
        }

        if (process_start_time.elapsed().as_secs() as i64) >= timeout.num_seconds() {
            break;
        }
    }

    // Wait for the process to exit before reaching the timeout
    // If not, we just kill it
    let exit_status;
    loop {
        match child_process.try_wait() {
            Ok(Some(status)) => {
                exit_status = status;
                break;
            }
            Ok(None) => {
                if (process_start_time.elapsed().as_secs() as i64) < timeout.num_seconds() {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    continue;
                }

                // Timeout !
                warn!(
                    "Killing process {} due to timeout {}m reached",
                    command_string,
                    timeout.num_minutes()
                );
                let _ = child_process
                    .kill() //Fire
                    .map(|_| child_process.wait())
                    .map_err(|err| error!("Cannot kill process {:?} {}", child_process, err));

                return Err(SimpleError::new(
                    Other,
                    Some(format!("Image build timeout after {} seconds", timeout.num_seconds())),
                ));
            }
            Err(err) => return Err(SimpleError::from(err)),
        };
    }

    // Process exited
    if exit_status.success() {
        return Ok(command_output);
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
        &vec![],
        |r_out| match r_out {
            Ok(s) => output_from_cmd.push_str(&s),
            Err(e) => error!("Error while getting stdout from {} {}", binary_name, e),
        },
        |r_err| match r_err {
            Ok(_) => error!("Error executing {}", binary_name),
            Err(e) => error!("Error while getting stderr from {} {}", binary_name, e),
        },
        Duration::seconds(10),
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

pub fn command_to_string<P>(binary: P, args: &[&str], envs: &[(&str, &str)]) -> String
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

#[cfg(test)]
mod tests {
    use crate::cmd::utilities::exec_with_output;
    use chrono::Duration;

    #[test]
    fn test_command_with_timeout() {
        let ret = exec_with_output("sleep", vec!["120"], &vec![], |_| {}, |_| {}, Duration::seconds(2));
        assert_eq!(ret.is_err(), true);
        assert_eq!(ret.err().unwrap().message.unwrap().contains("timeout"), true);

        let ret = exec_with_output("yes", vec![""], &vec![], |_| {}, |_| {}, Duration::seconds(2));
        assert_eq!(ret.is_err(), true);

        let ret2 = exec_with_output("sleep", vec!["1"], &vec![], |_| {}, |_| {}, Duration::seconds(5));

        assert_eq!(ret2.is_ok(), true);
    }
}

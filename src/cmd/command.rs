use std::ffi::OsStr;
use std::io::{BufRead, BufReader};
use std::io::{Error, ErrorKind};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};

use crate::cmd::command::CommandError::{ExecutionError, ExitStatusError, Killed, TimeoutError};
use crate::cmd::command::CommandOutputType::{STDERR, STDOUT};

use chrono::Duration;
use itertools::Itertools;
use std::time::Instant;
use timeout_readwrite::TimeoutReader;

enum CommandOutputType {
    STDOUT(Result<String, std::io::Error>),
    STDERR(Result<String, std::io::Error>),
}

#[derive(thiserror::Error, Debug)]
pub enum CommandError {
    #[error("Error while executing command")]
    ExecutionError(#[from] std::io::Error),

    #[error("Command terminated with a non success exit status code: {0}")]
    ExitStatusError(ExitStatus),

    #[error("Command killed due to timeout: {0}")]
    TimeoutError(String),

    #[error("Command killed by user request: {0}")]
    Killed(String),
}

impl CommandError {
    pub fn to_string(&self) -> String {
        match self {
            ExecutionError(err) => format!("Execution error: {}", err.to_string()),
            ExitStatusError(exit_status) => {
                format!("Execution error: exit status {}", exit_status.to_string())
            }
            TimeoutError(msg) => format!("Execution error: timeout, {}", msg.to_string()),
            Killed(msg) => format!("Execution error: killed, {}", msg.to_string()),
        }
    }
}

pub struct QoveryCommand {
    command: Command,
}

impl QoveryCommand {
    pub fn new<P: AsRef<Path>>(binary: P, args: &[&str], envs: &[(&str, &str)]) -> QoveryCommand {
        let mut command = Command::new(binary.as_ref().as_os_str());
        command.args(args);

        envs.iter().for_each(|(k, v)| {
            command.env(k, v);
        });

        QoveryCommand { command }
    }

    pub fn set_current_dir<P: AsRef<Path>>(&mut self, root_dir: P) {
        self.command.current_dir(root_dir);
    }

    fn kill(cmd_handle: &mut Child) {
        let _ = cmd_handle
            .kill() //Fire
            .map(|_| cmd_handle.wait())
            .map_err(|err| error!("Cannot kill process {:?} {}", cmd_handle, err));
    }

    pub fn exec(&mut self) -> Result<(), CommandError> {
        self.exec_with_abort(
            Duration::max_value(),
            |line| info!("{}", line),
            |line| warn!("{}", line),
            || false,
        )
    }

    pub fn exec_with_output<STDOUT, STDERR>(
        &mut self,
        stdout_output: STDOUT,
        stderr_output: STDERR,
    ) -> Result<(), CommandError>
    where
        STDOUT: FnMut(String),
        STDERR: FnMut(String),
    {
        self.exec_with_abort(Duration::max_value(), stdout_output, stderr_output, || false)
    }

    pub fn exec_with_timeout<STDOUT, STDERR>(
        &mut self,
        timeout: Duration,
        stdout_output: STDOUT,
        stderr_output: STDERR,
    ) -> Result<(), CommandError>
    where
        STDOUT: FnMut(String),
        STDERR: FnMut(String),
    {
        self.exec_with_abort(timeout, stdout_output, stderr_output, || false)
    }

    pub fn exec_with_abort<STDOUT, STDERR, F>(
        &mut self,
        timeout: Duration,
        mut stdout_output: STDOUT,
        mut stderr_output: STDERR,
        should_be_killed: F,
    ) -> Result<(), CommandError>
    where
        STDOUT: FnMut(String),
        STDERR: FnMut(String),
        F: Fn() -> bool,
    {
        assert!(timeout.num_seconds() > 0, "Timeout cannot be a 0 or negative duration");

        info!("command: {:?}", self.command);
        let mut cmd_handle = self
            .command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(ExecutionError)?;

        let process_start_time = Instant::now();

        // Read stdout/stderr until timeout is reached
        let reader_timeout = std::time::Duration::from_secs(10.min(timeout.num_seconds() as u64));
        let stdout = cmd_handle.stdout.take().ok_or(ExecutionError(Error::new(
            ErrorKind::BrokenPipe,
            "Cannot get stdout for command",
        )))?;
        let stdout_reader = BufReader::new(TimeoutReader::new(stdout, reader_timeout))
            .lines()
            .map(STDOUT);

        let stderr = cmd_handle.stderr.take().ok_or(ExecutionError(Error::new(
            ErrorKind::BrokenPipe,
            "Cannot get stderr for command",
        )))?;
        let stderr_reader = BufReader::new(TimeoutReader::new(
            stderr,
            std::time::Duration::from_secs(0), // don't block on stderr
        ))
        .lines()
        .map(STDERR);

        for line in stdout_reader.interleave(stderr_reader) {
            match line {
                STDOUT(Err(ref err)) | STDERR(Err(ref err)) if err.kind() == ErrorKind::TimedOut => {}
                STDOUT(Ok(line)) => stdout_output(line),
                STDERR(Ok(line)) => stderr_output(line),
                STDOUT(Err(err)) => error!("Error on stdout of cmd {:?}: {:?}", self.command, err),
                STDERR(Err(err)) => error!("Error on stderr of cmd {:?}: {:?}", self.command, err),
            }

            if should_be_killed() {
                break;
            }

            if (process_start_time.elapsed().as_secs() as i64) >= timeout.num_seconds() {
                break;
            }
        }

        // Wait for the process to exit before reaching the timeout
        // If not, we just kill it
        let exit_status;
        loop {
            match cmd_handle.try_wait() {
                Ok(Some(status)) => {
                    exit_status = status;
                    break;
                }
                Ok(None) => {
                    // Does the process should be killed ?
                    if should_be_killed() {
                        let msg = format!("Killing process {:?}", self.command);
                        warn!("{}", msg);
                        Self::kill(&mut cmd_handle);
                        return Err(Killed(msg));
                    }

                    // Does the timeout has been reached ?
                    if (process_start_time.elapsed().as_secs() as i64) >= timeout.num_seconds() {
                        let msg = format!(
                            "Killing process {:?} due to timeout {}m reached",
                            self.command,
                            timeout.num_minutes()
                        );
                        warn!("{}", msg);
                        Self::kill(&mut cmd_handle);
                        return Err(TimeoutError(msg));
                    }
                }
                Err(err) => return Err(ExecutionError(err)),
            };

            // Sleep a bit and retry to check
            std::thread::sleep(std::time::Duration::from_secs(1));
        }

        if !exit_status.success() {
            debug!(
                "command: {:?} terminated with error exist status {:?}",
                self.command, exit_status
            );
            return Err(ExitStatusError(exit_status));
        }

        Ok(())
    }
}

// return the output of "binary_name" --version
pub fn run_version_command_for(binary_name: &str) -> String {
    let mut output_from_cmd = String::new();
    let mut cmd = QoveryCommand::new(binary_name, &vec!["--version"], Default::default());
    let _ = cmd.exec_with_output(
        |r_out| output_from_cmd.push_str(&r_out),
        |r_err| error!("Error executing {}: {}", binary_name, r_err),
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
    let _envs = envs.iter().map(|(k, v)| format!("{}={}", k, v)).join(" ");
    format!("{} {:?} {}", _envs, binary.as_ref().as_os_str(), args.join(" "))
}

#[cfg(test)]
mod tests {
    use crate::cmd::command::{does_binary_exist, run_version_command_for, CommandError, QoveryCommand};
    use chrono::Duration;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Barrier};
    use std::{thread, time};

    #[test]
    fn test_binary_exist() {
        assert_eq!(does_binary_exist("sdfsdf"), false);
        assert_eq!(does_binary_exist("ls"), true);
        assert_eq!(does_binary_exist("/bin/sh"), true);
    }

    #[test]
    fn test_run_version_for_command() {
        let ret = run_version_command_for("ls");
        assert_eq!(ret.is_empty(), false);
        assert_eq!(ret.contains("GNU"), true)
    }

    #[test]
    fn test_error() {
        let mut cmd = QoveryCommand::new("false", &vec![], &vec![]);
        assert_eq!(cmd.exec().is_err(), true);
        assert_eq!(matches!(cmd.exec(), Err(CommandError::ExitStatusError(_))), true);
    }

    #[test]
    fn test_command_with_timeout() {
        let mut cmd = QoveryCommand::new("sleep", &vec!["120"], &vec![]);
        let ret = cmd.exec_with_timeout(Duration::seconds(2), |_| {}, |_| {});

        assert!(matches!(ret, Err(CommandError::TimeoutError(_))));

        let mut cmd = QoveryCommand::new("sh", &vec!["-c", "cat /dev/urandom | grep -a --null-data ."], &vec![]);
        let ret = cmd.exec_with_timeout(Duration::seconds(2), |_| {}, |_| {});

        assert!(matches!(ret, Err(CommandError::TimeoutError(_))));

        let mut cmd = QoveryCommand::new("sleep", &vec!["1"], &vec![]);
        let ret = cmd.exec_with_timeout(Duration::seconds(2), |_| {}, |_| {});
        assert_eq!(ret.is_ok(), true);
    }

    #[test]
    fn test_command_with_abort() {
        let mut cmd = QoveryCommand::new("sleep", &vec!["120"], &vec![]);
        let should_kill = Arc::new(AtomicBool::new(false));
        let should_kill2 = should_kill.clone();
        let barrier = Arc::new(Barrier::new(2));

        let _ = thread::spawn({
            let barrier = barrier.clone();
            move || {
                barrier.wait();
                thread::sleep(time::Duration::from_secs(2));
                should_kill.store(true, Ordering::Release);
            }
        });

        let cmd_killer = move || should_kill2.load(Ordering::Acquire);
        barrier.wait();
        let ret = cmd.exec_with_abort(Duration::max_value(), |_| {}, |_| {}, cmd_killer);

        assert!(matches!(ret, Err(CommandError::Killed(_))));
    }
}

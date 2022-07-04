use std::ffi::OsStr;
use std::io::{BufRead, BufReader};
use std::io::{Error, ErrorKind};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};

use crate::cmd::command::CommandError::{ExecutionError, ExitStatusError, Killed, TimeoutError};

use itertools::Itertools;
use std::time::{Duration, Instant};
use timeout_readwrite::TimeoutReader;

#[derive(thiserror::Error, Debug)]
pub enum CommandError {
    #[error("Error while executing command")]
    ExecutionError(#[from] Error),

    #[error("Command terminated with a non success exit status code: {0}")]
    ExitStatusError(ExitStatus),

    #[error("Command killed due to timeout: {0}")]
    TimeoutError(String),

    #[error("Command killed by user request: {0}")]
    Killed(String),
}

#[derive(Debug, Clone)]
pub enum AbortReason {
    Timeout(Duration),
    Canceled(String),
}
pub struct CommandKiller<'a> {
    should_abort: Box<dyn Fn() -> Option<AbortReason> + 'a>,
}

impl<'a> CommandKiller<'a> {
    pub fn never() -> CommandKiller<'a> {
        CommandKiller {
            should_abort: Box::new(|| None),
        }
    }

    pub fn from_timeout(timeout: Duration) -> CommandKiller<'a> {
        let now = Instant::now();
        CommandKiller {
            should_abort: Box::new(move || {
                if now.elapsed() >= timeout {
                    return Some(AbortReason::Timeout(timeout));
                }

                None
            }),
        }
    }

    pub fn from_cancelable(is_canceled: &'a dyn Fn() -> bool) -> CommandKiller<'a> {
        CommandKiller {
            should_abort: Box::new(move || {
                if is_canceled() {
                    return Some(AbortReason::Canceled("Task canceled".to_string()));
                }
                None
            }),
        }
    }

    pub fn from(timeout: Duration, is_canceled: &'a dyn Fn() -> bool) -> CommandKiller<'a> {
        let has_timeout = Self::from_timeout(timeout);
        let is_canceled = Self::from_cancelable(is_canceled);
        CommandKiller {
            should_abort: Box::new(move || {
                if let Some(reason) = (has_timeout.should_abort)() {
                    return Some(reason);
                }

                if let Some(reason) = (is_canceled.should_abort)() {
                    return Some(reason);
                }

                None
            }),
        }
    }

    pub fn should_abort(&self) -> Option<AbortReason> {
        (self.should_abort)()
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
            &mut |line| info!("{}", line),
            &mut |line| warn!("{}", line),
            &CommandKiller::never(),
        )
    }

    pub fn exec_with_output<STDOUT, STDERR>(
        &mut self,
        stdout_output: &mut STDOUT,
        stderr_output: &mut STDERR,
    ) -> Result<(), CommandError>
    where
        STDOUT: FnMut(String),
        STDERR: FnMut(String),
    {
        self.exec_with_abort(stdout_output, stderr_output, &CommandKiller::never())
    }

    pub fn exec_with_abort<STDOUT, STDERR>(
        &mut self,
        stdout_output: &mut STDOUT,
        stderr_output: &mut STDERR,
        abort_notifier: &CommandKiller,
    ) -> Result<(), CommandError>
    where
        STDOUT: FnMut(String),
        STDERR: FnMut(String),
    {
        info!("command: {:?}", self.command);
        let mut cmd_handle = self
            .command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(ExecutionError)?;

        // Read stdout/stderr until timeout is reached
        let reader_timeout = Duration::from_secs(1);
        let stdout = cmd_handle
            .stdout
            .take()
            .ok_or_else(|| ExecutionError(Error::new(ErrorKind::BrokenPipe, "Cannot get stdout for command")))?;
        let mut stdout_reader = BufReader::new(TimeoutReader::new(stdout, reader_timeout)).lines();

        let stderr = cmd_handle
            .stderr
            .take()
            .ok_or_else(|| ExecutionError(Error::new(ErrorKind::BrokenPipe, "Cannot get stderr for command")))?;
        let mut stderr_reader = BufReader::new(TimeoutReader::new(
            stderr,
            Duration::from_secs(0), // don't block on stderr
        ))
        .lines();

        let mut stdout_closed = false;
        let mut stderr_closed = false;
        while !stdout_closed || !stderr_closed {
            // We should abort and kill the process
            if abort_notifier.should_abort().is_some() {
                break;
            }

            // Read on stdout first
            while !stdout_closed {
                let line = match stdout_reader.next() {
                    Some(line) => line,
                    None => {
                        // Stdout has been closed
                        stdout_closed = true;
                        break;
                    }
                };

                match line {
                    Err(ref err) if err.kind() == ErrorKind::TimedOut => break,
                    Ok(line) => stdout_output(line),
                    Err(err) => {
                        error!("Error on stdout of cmd {:?}: {:?}", self.command, err);
                        stdout_closed = true;
                        break;
                    }
                }

                // Should we abort and kill the process
                if abort_notifier.should_abort().is_some() {
                    stdout_closed = true;
                    stderr_closed = true;
                    break;
                }
            }

            // Read stderr now
            while !stderr_closed {
                let line = match stderr_reader.next() {
                    Some(line) => line,
                    None => {
                        // Stdout has been closed
                        stderr_closed = true;
                        break;
                    }
                };

                match line {
                    Err(ref err) if err.kind() == ErrorKind::TimedOut => break,
                    Ok(line) => stderr_output(line),
                    Err(err) => {
                        error!("Error on stderr of cmd {:?}: {:?}", self.command, err);
                        stderr_closed = true;
                        break;
                    }
                }

                // should we abort and kill the process
                if abort_notifier.should_abort().is_some() {
                    stdout_closed = true;
                    stderr_closed = true;
                    break;
                }
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
                    match abort_notifier.should_abort() {
                        None => {}
                        Some(AbortReason::Timeout(timeout)) => {
                            let msg = format!(
                                "Killing process {:?} due to timeout {}s reached",
                                self.command,
                                timeout.as_secs()
                            );
                            warn!("{}", msg);
                            Self::kill(&mut cmd_handle);
                            return Err(TimeoutError(msg));
                        }
                        Some(AbortReason::Canceled(_)) => {
                            let msg = format!("Killing process {:?}", self.command);
                            warn!("{}", msg);
                            Self::kill(&mut cmd_handle);
                            return Err(Killed(msg));
                        }
                    }
                }
                Err(err) => return Err(ExecutionError(err)),
            };

            // Sleep a bit and retry to check
            std::thread::sleep(Duration::from_secs(1));
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
    let mut cmd = QoveryCommand::new(binary_name, &["--version"], Default::default());
    let _ = cmd.exec_with_output(&mut |r_out| output_from_cmd.push_str(&r_out), &mut |r_err| {
        error!("Error executing {}: {}", binary_name, r_err)
    });

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
    use crate::cmd::command::{does_binary_exist, run_version_command_for, CommandError, CommandKiller, QoveryCommand};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::Duration;

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
        assert!(ret.contains("GNU"))
    }

    #[test]
    fn test_error() {
        let mut cmd = QoveryCommand::new("false", &[], &[]);
        assert_eq!(cmd.exec().is_err(), true);
        assert!(matches!(cmd.exec(), Err(CommandError::ExitStatusError(_))));
    }

    #[test]
    fn test_command_with_timeout() {
        let mut cmd = QoveryCommand::new("sleep", &["120"], &[]);
        let ret = cmd.exec_with_abort(&mut |_| {}, &mut |_| {}, &CommandKiller::from_timeout(Duration::from_secs(2)));

        assert!(matches!(ret, Err(CommandError::TimeoutError(_))));

        let mut cmd = QoveryCommand::new("sh", &["-c", "cat /dev/urandom | grep -a --null-data ."], &[]);
        let ret = cmd.exec_with_abort(&mut |_| {}, &mut |_| {}, &CommandKiller::from_timeout(Duration::from_secs(2)));

        assert!(matches!(ret, Err(CommandError::TimeoutError(_))));

        let mut cmd = QoveryCommand::new("sleep", &["1"], &[]);
        let ret = cmd.exec_with_abort(&mut |_| {}, &mut |_| {}, &CommandKiller::from_timeout(Duration::from_secs(2)));
        assert!(ret.is_ok());
    }

    #[test]
    fn test_command_with_abort() {
        let mut cmd = QoveryCommand::new("sleep", &["120"], &[]);
        let should_kill = Arc::new(AtomicBool::new(false));
        let should_kill2 = should_kill.clone();
        let barrier = Arc::new(Barrier::new(2));

        let _ = thread::spawn({
            let barrier = barrier.clone();
            move || {
                barrier.wait();
                thread::sleep(Duration::from_secs(2));
                should_kill.store(true, Ordering::Release);
            }
        });

        let cmd_killer = move || should_kill2.load(Ordering::Acquire);
        let cmd_killer = CommandKiller::from_cancelable(&cmd_killer);
        barrier.wait();
        let ret = cmd.exec_with_abort(&mut |_| {}, &mut |_| {}, &cmd_killer);

        assert!(matches!(ret, Err(CommandError::Killed(_))));
    }
}

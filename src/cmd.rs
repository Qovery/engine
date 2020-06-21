use std::io::Error;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};

pub fn exec<P, F>(binary: P, args: Vec<&str>, output: F) -> Result<ExitStatus, Error>
where
    P: AsRef<Path>,
    F: Fn(Result<String, Error>),
{
    let mut cmd = Command::new(binary.as_ref())
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let stdout_reader = BufReader::new(cmd.stdout.as_mut().unwrap());
    let stderr_reader = BufReader::new(cmd.stderr.as_mut().unwrap());

    for line in stdout_reader.lines() {
        output(line);
    }

    for line in stderr_reader.lines() {
        output(line);
    }

    cmd.wait()
}

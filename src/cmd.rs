use std::io::Error;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};

fn get_child<P>(binary: P, args: Vec<&str>) -> Child
where
    P: AsRef<Path>,
{
    Command::new(binary.as_ref())
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

pub fn exec<P>(binary: P, args: Vec<&str>) -> Result<ExitStatus, Error>
where
    P: AsRef<Path>,
{
    get_child(binary, args).wait()
}

pub fn exec_with_output<P, F>(binary: P, args: Vec<&str>, output: F) -> Result<ExitStatus, Error>
where
    P: AsRef<Path>,
    F: Fn(Result<String, Error>),
{
    let mut child = get_child(binary, args);

    let stdout_reader = BufReader::new(child.stdout.as_mut().unwrap());
    let stderr_reader = BufReader::new(child.stderr.as_mut().unwrap());

    for line in stdout_reader.lines() {
        output(line);
    }

    for line in stderr_reader.lines() {
        output(line);
    }

    child.wait()
}

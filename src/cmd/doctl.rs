use crate::cmd::utilities::{exec_with_envs_and_output, exec_with_output};
use crate::error::{SimpleError, SimpleErrorKind};
use std::io::Error;

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Root {
    pub errors: Vec<ErrorDoctl>,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(rename = "error")]
pub struct ErrorDoctl {
    pub detail: String,
}

pub fn doctl_do_registry_login(token: &str) -> Result<(), SimpleError> {
    let mut output_string = String::new();
    let _ = doctl_exec_with_output(
        vec!["registry", "login", "-t", token],
        |out| match out {
            Ok(line) => {}
            Err(err) => error!("{:?}", err),
        },
        |out| match out {
            Ok(line) => output_string = line,
            Err(err) => error!("{:?}", err),
        },
    );
    if output_string.contains("412") {
        warn!("Digital Ocean account doesn't contains registry");
    }
    if output_string.contains("401") {
        return Err(SimpleError::new(
            SimpleErrorKind::Other,
            Some(output_string),
        ));
    }
    Ok(())
}

pub fn doctl_do_registry_get_repository(token: &str) -> Result<(), SimpleError> {
    let mut output_string = String::new();
    doctl_exec_with_output(
        vec!["registry", "get", "-t", token, "--output", "json"],
        |out| match out {
            Ok(line) => {}
            Err(err) => error!("{:?}", err),
        },
        |out| match out {
            Ok(line) => output_string = line,
            Err(err) => error!("{:?}", err),
        },
    );
    let mut res = match serde_json::from_str::<Vec<ErrorDoctl>>(output_string.as_str()) {
        Ok(x) => x,
        Err(_) => vec![],
    };
    // TODO finish me

    Ok(())
}

pub fn doctl_do_registry_create(token: &str) -> Result<(), SimpleError> {
    let _ = doctl_exec_with_output(
        vec!["registry", "create", "qovery", "-t", token],
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

pub fn doctl_exec_with_output<F, X>(
    args: Vec<&str>,
    stdout_output: F,
    stderr_output: X,
) -> Result<(), SimpleError>
where
    F: FnMut(Result<String, Error>),
    X: FnMut(Result<String, Error>),
{
    match exec_with_output("doctl", args, stdout_output, stderr_output) {
        Err(err) => return Err(err),
        _ => {}
    };

    Ok(())
}

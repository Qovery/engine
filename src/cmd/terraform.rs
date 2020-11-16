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
use serde_json::Value;

use crate::cmd::utilities::exec_with_envs_and_output;
use crate::constants::{KUBECONFIG, TF_PLUGIN_CACHE_DIR};
use crate::error::{SimpleError, SimpleErrorKind};

fn terraform_exec_with_init_validate(root_dir: &str) -> Result<(), SimpleError> {
    // terraform init
    let result = retry::retry(Fibonacci::from_millis(3000).take(5), || {
        let try_result = terraform_exec(root_dir, vec!["init"]);
        match try_result {
            Ok(out) => OperationResult::Ok(out),
            Err(err) => OperationResult::Err(format!("Command error: {:?}", err)),
        }
    });

    match result {
        Err(err) => match err {
            retry::Error::Operation {
                error: _,
                total_delay: _,
                tries: _,
            } => Ok(Some(false)),
            retry::Error::Internal(err) => Err(SimpleError::new(SimpleErrorKind::Other, Some(err))),
        },
        Ok(_) => Ok(Some(true)),
    };

    // terraform validate config
    terraform_exec(root_dir, vec!["validate"])?;

    Ok(())
}

fn terraform_exec_with_init_validate_plan(
    root_dir: &str,
    first_time_init_terraform: bool,
) -> Result<(), SimpleError> {
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
    dry_run: bool,
) -> Result<(), SimpleError> {
    // terraform init and plan
    terraform_exec_with_init_validate_plan(root_dir, first_time_init_terraform);

    // terraform apply
    if !dry_run {
        terraform_exec(root_dir, vec!["apply", "-auto-approve", "tf_plan"])?;
    }

    Ok(())
}

pub fn terraform_exec_with_init_plan_apply_destroy(root_dir: &str) -> Result<(), SimpleError> {
    // terraform init and plan
    // should apply before destroy to be sure destroy will compute on all ressources
    terraform_exec_with_init_validate_plan_apply(root_dir, false, false);

    // terraform destroy
    terraform_exec(root_dir, vec!["destroy", "-auto-approve"])
}

pub fn terraform_exec(root_dir: &str, args: Vec<&str>) -> Result<(), SimpleError> {
    let home_dir = home_dir().expect("Could not find $HOME");
    let tf_plugin_cache_dir = format!("{}/.terraform.d/plugin-cache", home_dir.to_str().unwrap());

    exec_with_envs_and_output(
        format!("{} terraform", root_dir).as_str(),
        args,
        vec![(TF_PLUGIN_CACHE_DIR, tf_plugin_cache_dir.as_str())],
        |line: Result<String, std::io::Error>| {
            info!("{}", line.unwrap());
        },
        |line: Result<String, std::io::Error>| {
            error!("{}", line.unwrap());
        },
    )
}

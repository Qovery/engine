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

use crate::cmd::utilities::{exec_with_envs_and_output, CmdError};
use crate::constants::{KUBECONFIG, TF_PLUGIN_CACHE_DIR};

fn terraform_exec_with_init_validate(
    root_dir: &str,
    first_time_init_terraform: bool,
) -> Result<(), CmdError> {
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

    Ok(())
}

fn terraform_exec_with_init_validate_plan(
    root_dir: &str,
    first_time_init_terraform: bool,
) -> Result<(), CmdError> {
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
) -> Result<(), CmdError> {
    // terraform init and plan
    terraform_exec_with_init_validate_plan(root_dir, first_time_init_terraform);

    // terraform apply
    terraform_exec(root_dir, vec!["apply", "-auto-approve", "tf_plan"])?;

    Ok(())
}

pub fn terraform_exec_with_init_validate_destroy(root_dir: &str) -> Result<(), CmdError> {
    // terraform init and plan
    terraform_exec_with_init_validate(root_dir, false);

    // terraform destroy
    terraform_exec(root_dir, vec!["destroy", "-auto-approve"])
}

pub fn terraform_exec(root_dir: &str, args: Vec<&str>) -> Result<(), CmdError> {
    let home_dir = home_dir().expect("Could not find $HOME");
    let tf_plugin_cache_dir = format!("{}/.terraform.d/plugin-cache", home_dir.to_str().unwrap());

    let result = retry::retry(Fibonacci::from_millis(3000).take(5), || {
        let r = exec_with_envs_and_output(
            format!("{} terraform", root_dir).as_str(),
            args.clone(),
            vec![(TF_PLUGIN_CACHE_DIR, tf_plugin_cache_dir.as_str())],
            |line: Result<String, std::io::Error>| {
                info!("{}", line.unwrap());
            },
            |line: Result<String, std::io::Error>| {
                error!("{}", line.unwrap());
            },
        );
        match r {
            Ok(terra_well) => OperationResult::Ok(terra_well),
            Err(terra_nok) => OperationResult::Err(format!("command error: {:?}", terra_nok)),
        }
    });

    match result {
        Err(err) => {
            return Err(CmdError::Unexpected(
                "Unable to make Terraform command works despite of multiple attemps".to_string(),
            ));
        }

        Ok(_) => Ok(()),
    }
}

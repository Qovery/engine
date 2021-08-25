use dirs::home_dir;
use retry::delay::Fixed;
use retry::OperationResult;

use crate::cmd::utilities::exec_with_envs_and_output;
use crate::constants::TF_PLUGIN_CACHE_DIR;
use crate::error::{SimpleError, SimpleErrorKind};
use chrono::Duration;
use rand::Rng;
use retry::Error::Operation;
use std::{thread, time};

fn terraform_init_validate(root_dir: &str) -> Result<(), SimpleError> {
    // terraform init
    let result = retry::retry(Fixed::from_millis(3000).take(5), || {
        match terraform_exec(root_dir, vec!["init"]) {
            Ok(out) => OperationResult::Ok(out),
            Err(err) => {
                // Error: Failed to install provider from shared cache
                // in order to avoid lock errors on parallel run, let's sleep a bit
                if err.message.is_some() {
                    let message = err.message.clone();
                    if message
                        .unwrap()
                        .contains("Failed to install provider from shared cache")
                    {
                        let sleep_time_int = rand::thread_rng().gen_range(30..75);
                        let sleep_time = time::Duration::from_millis(sleep_time_int);
                        info!(
                            "another terraform command is trying to use shared provider cache which is forbidden, sleeping {} before retrying...",
                            sleep_time_int
                        );
                        thread::sleep(sleep_time);
                    }
                };
                error!("error while trying to run terraform init, retrying...");
                OperationResult::Retry(err)
            }
        }
    });

    match result {
        Ok(_) => {}
        Err(Operation { error, .. }) => return Err(error),
        Err(retry::Error::Internal(e)) => return Err(SimpleError::new(SimpleErrorKind::Other, Some(e))),
    }

    // validate config
    match terraform_exec(root_dir, vec!["validate"]) {
        Err(e) => {
            error!("error while trying to Terraform validate the rendered templates");
            return Err(e);
        }
        Ok(_) => Ok(()),
    }
}

pub fn terraform_init_validate_plan_apply(root_dir: &str, dry_run: bool) -> Result<(), SimpleError> {
    match terraform_init_validate(root_dir) {
        Err(e) => return Err(e),
        Ok(_) => {}
    }

    if dry_run {
        // plan
        let result = retry::retry(Fixed::from_millis(3000).take(3), || {
            match terraform_exec(root_dir, vec!["plan", "-out", "tf_plan"]) {
                Ok(out) => OperationResult::Ok(out),
                Err(err) => {
                    error!("While trying to Terraform plan the rendered templates");
                    OperationResult::Retry(err)
                }
            }
        });

        return match result {
            Ok(_) => Ok(()),
            Err(Operation { error, .. }) => Err(error),
            Err(retry::Error::Internal(e)) => Err(SimpleError::new(SimpleErrorKind::Other, Some(e))),
        };
    }

    match terraform_plan_apply(root_dir) {
        Ok(_) => Ok(()),
        Err(e) => Err(e),
    }
}

pub fn terraform_init_validate_destroy(root_dir: &str, run_apply_before_destroy: bool) -> Result<(), SimpleError> {
    // terraform init
    match terraform_init_validate(root_dir) {
        Err(e) => return Err(e),
        Ok(_) => {}
    }

    // better to apply before destroy to ensure terraform destroy will delete on all resources
    if run_apply_before_destroy {
        match terraform_plan_apply(root_dir) {
            Ok(_) => {}
            Err(e) => return Err(e),
        }
    }

    // terraform destroy
    let result = retry::retry(Fixed::from_millis(3000).take(5), || {
        match terraform_exec(root_dir, vec!["destroy", "-auto-approve"]) {
            Ok(out) => OperationResult::Ok(out),
            Err(err) => {
                error!("error while trying to run terraform destroy on rendered templates, retrying...");
                OperationResult::Retry(err)
            }
        }
    });

    match result {
        Ok(_) => Ok(()),
        Err(Operation { error, .. }) => Err(error),
        Err(retry::Error::Internal(e)) => Err(SimpleError::new(SimpleErrorKind::Other, Some(e))),
    }
}

fn terraform_plan_apply(root_dir: &str) -> Result<(), SimpleError> {
    let result = retry::retry(Fixed::from_millis(3000).take(5), || {
        // plan
        match terraform_exec(root_dir, vec!["plan", "-out", "tf_plan"]) {
            Ok(_) => {}
            Err(err) => {
                error!("While trying to Terraform plan the rendered templates");
                return OperationResult::Retry(err);
            }
        };
        // apply
        match terraform_exec(root_dir, vec!["apply", "-auto-approve", "tf_plan"]) {
            Ok(out) => OperationResult::Ok(out),
            Err(err) => {
                error!("error while trying to run terraform apply on rendered templates, retrying...");
                OperationResult::Retry(err)
            }
        }
    });

    match result {
        Ok(_) => Ok(()),
        Err(Operation { error, .. }) => Err(error),
        Err(retry::Error::Internal(e)) => Err(SimpleError::new(SimpleErrorKind::Other, Some(e))),
    }
}

pub fn terraform_init_validate_state_list(root_dir: &str) -> Result<Vec<String>, SimpleError> {
    // terraform init and validate
    match terraform_init_validate(root_dir) {
        Err(e) => return Err(e),
        Ok(_) => {}
    }

    // get terraform state list output
    let result = retry::retry(Fixed::from_millis(3000).take(5), || {
        match terraform_exec(root_dir, vec!["state", "list"]) {
            Ok(out) => OperationResult::Ok(out),
            Err(err) => {
                error!("error while trying to run terraform state list, retrying...");
                OperationResult::Retry(err)
            }
        }
    });

    match result {
        Ok(output) => Ok(output),
        Err(Operation { error, .. }) => Err(error),
        Err(retry::Error::Internal(e)) => Err(SimpleError::new(SimpleErrorKind::Other, Some(e))),
    }
}

pub fn terraform_exec(root_dir: &str, args: Vec<&str>) -> Result<Vec<String>, SimpleError> {
    let home_dir = home_dir().expect("Could not find $HOME");
    let tf_plugin_cache_dir = format!("{}/.terraform.d/plugin-cache", home_dir.to_str().unwrap());

    let result = exec_with_envs_and_output(
        format!("{} terraform", root_dir).as_str(),
        args,
        vec![(TF_PLUGIN_CACHE_DIR, tf_plugin_cache_dir.as_str())],
        |line: Result<String, std::io::Error>| {
            let output = line.unwrap();
            info!("{}", &output)
        },
        |line: Result<String, std::io::Error>| {
            let output = line.unwrap();
            error!("{}", &output);
        },
        Duration::max_value(),
    );

    match result {
        Ok(_) => Ok(result.unwrap()),
        Err(e) => Err(e),
    }
}

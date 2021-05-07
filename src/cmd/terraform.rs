use dirs::home_dir;
use retry::delay::Fixed;
use retry::OperationResult;

use crate::cmd::utilities::exec_with_envs_and_output;
use crate::constants::TF_PLUGIN_CACHE_DIR;
use crate::error::{SimpleErrorKind, TerraformError};
use chrono::Duration;
use retry::Error::Operation;

fn terraform_init_validate(root_dir: &str) -> Result<(), TerraformError> {
    // terraform init
    let result = retry::retry(Fixed::from_millis(3000).take(5), || {
        match terraform_exec(root_dir, vec!["init"]) {
            Ok(out) => OperationResult::Ok(out),
            Err(err) => {
                error!("error while trying to run terraform init, retrying...");
                OperationResult::Retry(err)
            }
        }
    });

    match result {
        Ok(_) => {}
        Err(Operation { error, .. }) => return Err(error),
        Err(retry::Error::Internal(e)) => return Err(TerraformError::new(SimpleErrorKind::Other, Some(e), None)),
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

pub fn terraform_init_validate_plan_apply(root_dir: &str, dry_run: bool) -> Result<(), TerraformError> {
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
            Err(retry::Error::Internal(e)) => Err(TerraformError::new(SimpleErrorKind::Other, Some(e), None)),
        };
    }

    match terraform_plan_apply(root_dir) {
        Ok(_) => Ok(()),
        Err(e) => Err(e),
    }
}

pub fn terraform_init_validate_destroy(root_dir: &str, run_apply_before_destroy: bool) -> Result<(), TerraformError> {
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
        Err(retry::Error::Internal(e)) => Err(TerraformError::new(SimpleErrorKind::Other, Some(e), None)),
    }
}

fn terraform_plan_apply(root_dir: &str) -> Result<(), TerraformError> {
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
        Err(retry::Error::Internal(e)) => Err(TerraformError::new(SimpleErrorKind::Other, Some(e), None)),
    }
}

pub fn terraform_init_validate_state_list(root_dir: &str) -> Result<Vec<String>, TerraformError> {
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
        Err(retry::Error::Internal(e)) => Err(TerraformError::new(SimpleErrorKind::Other, Some(e), None)),
    }
}

pub fn terraform_exec(root_dir: &str, args: Vec<&str>) -> Result<Vec<String>, TerraformError> {
    let home_dir = home_dir().expect("Could not find $HOME");
    let tf_plugin_cache_dir = format!("{}/.terraform.d/plugin-cache", home_dir.to_str().unwrap());
    let tf_error_pattern_match = "Error:";

    let mut tf_output = Vec::new();
    let mut tf_output_err = Vec::new();
    let result = exec_with_envs_and_output(
        format!("{} terraform", root_dir).as_str(),
        args,
        vec![(TF_PLUGIN_CACHE_DIR, tf_plugin_cache_dir.as_str())],
        |line: Result<String, std::io::Error>| {
            let output = line.unwrap();
            info!("{}", &output);
            // todo: ensure there is no useful other lines, and remove this if necessary
            if output.contains(tf_error_pattern_match) {
                tf_output.push(output);
            }
        },
        |line: Result<String, std::io::Error>| {
            let output = line.unwrap();
            error!("{}", &output);
            if output.contains(tf_error_pattern_match) {
                tf_output_err.push(output);
            }
        },
        Duration::max_value(),
    );

    tf_output.append(&mut tf_output_err);

    match result {
        Ok(_) => Ok(result.unwrap()),
        Err(e) => Err(TerraformError {
            kind: SimpleErrorKind::Other,
            message: e.message,
            logs: Some(tf_output),
        }),
    }
}

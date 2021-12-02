use dirs::home_dir;
use retry::delay::Fixed;
use retry::OperationResult;

use crate::cmd::utilities::QoveryCommand;
use crate::constants::TF_PLUGIN_CACHE_DIR;
use crate::error::SimpleErrorKind::Other;
use crate::error::{SimpleError, SimpleErrorKind};
use rand::Rng;
use retry::Error::Operation;
use std::{env, fs, thread, time};

fn manage_common_issues(terraform_provider_lock: &String, err: &SimpleError) -> bool {
    // Error: Failed to install provider from shared cache
    // in order to avoid lock errors on parallel run, let's sleep a bit
    // https://github.com/hashicorp/terraform/issues/28041
    let mut found_managed_issue = false;
    debug!("{:?}", err);

    match &err.message {
        None => warn!("no know method to fix this Terraform issue"),
        Some(message) => {
            if message.contains("Failed to install provider from shared cache")
                || message.contains("Failed to install provider")
            {
                found_managed_issue = true;
                let sleep_time_int = rand::thread_rng().gen_range(20..45);
                let sleep_time = time::Duration::from_secs(sleep_time_int);

                warn!(
                    "failed to install provider from shared cache, cleaning and sleeping {} before retrying...",
                    sleep_time_int
                );
                thread::sleep(sleep_time);

                match fs::remove_file(&terraform_provider_lock) {
                    Ok(_) => info!("terraform lock file {} has been removed", &terraform_provider_lock),
                    Err(e) => error!(
                        "wasn't able to delete terraform lock file {}: {}",
                        &terraform_provider_lock, e
                    ),
                }
            }

            if message.contains("Plugin reinitialization required") {
                warn!("terraform init is required");
                found_managed_issue = true;
            }
        }
    };

    found_managed_issue
}

fn terraform_init_validate(root_dir: &str) -> Result<(), SimpleError> {
    let terraform_provider_lock = format!("{}/.terraform.lock.hcl", &root_dir);

    let result = retry::retry(Fixed::from_millis(3000).take(5), || {
        // terraform init
        match terraform_exec(root_dir, vec!["init", "-no-color"]) {
            Ok(_) => OperationResult::Ok(()),
            Err(err) => {
                manage_common_issues(&terraform_provider_lock, &err);
                error!("error while trying to run terraform init, retrying...");
                OperationResult::Retry(err)
            }
        };

        // validate config
        match terraform_exec(root_dir, vec!["validate", "-no-color"]) {
            Ok(_) => OperationResult::Ok(()),
            Err(err) => {
                manage_common_issues(&terraform_provider_lock, &err);
                error!("error while trying to Terraform validate on the rendered templates");
                OperationResult::Retry(err)
            }
        }
    });

    match result {
        Ok(_) => Ok(()),
        Err(Operation { error, .. }) => return Err(error),
        Err(retry::Error::Internal(e)) => return Err(SimpleError::new(SimpleErrorKind::Other, Some(e))),
    }
}

pub fn terraform_init_validate_plan_apply(root_dir: &str, dry_run: bool) -> Result<(), SimpleError> {
    if let Err(e) = terraform_init_validate(root_dir) {
        return Err(e);
    }

    if dry_run {
        // plan
        let result = retry::retry(Fixed::from_millis(3000).take(3), || {
            match terraform_exec(root_dir, vec!["plan", "-no-color", "-out", "tf_plan"]) {
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
    if let Err(e) = terraform_init_validate(root_dir) {
        return Err(e);
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
        match terraform_exec(root_dir, vec!["destroy", "-no-color", "-auto-approve"]) {
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
        match terraform_exec(root_dir, vec!["plan", "-no-color", "-out", "tf_plan"]) {
            Ok(_) => {}
            Err(err) => {
                error!("While trying to Terraform plan the rendered templates");
                return OperationResult::Retry(err);
            }
        };
        // apply
        match terraform_exec(root_dir, vec!["apply", "-no-color", "-auto-approve", "tf_plan"]) {
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
    if let Err(e) = terraform_init_validate(root_dir) {
        return Err(e);
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
    // override if environment variable is set
    let tf_plugin_cache_dir_value = match env::var_os(TF_PLUGIN_CACHE_DIR) {
        Some(val) => format!("{:?}", val),
        None => {
            let home_dir = home_dir().expect("Could not find $HOME");
            format!("{}/.terraform.d/plugin-cache", home_dir.to_str().unwrap())
        }
    };

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut cmd = QoveryCommand::new(
        "terraform",
        &args,
        &vec![(TF_PLUGIN_CACHE_DIR, tf_plugin_cache_dir_value.as_str())],
    );
    cmd.set_current_dir(root_dir);

    let result = cmd.exec_with_output(
        |line| {
            info!("{}", line);
            stdout.push(line);
        },
        |line| {
            error!("{}", line);
            stderr.push(line);
        },
    );

    stdout.extend(stderr);

    match result {
        Ok(_) => Ok(stdout),
        Err(e) => {
            debug!("Terraform endend in error {:?}", e);
            let err = SimpleError::new(Other, Some(stdout.join("\n")));
            Err(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::cmd::terraform::{manage_common_issues, terraform_init_validate};
    use crate::error::{SimpleError, SimpleErrorKind};
    use std::fs;
    use tracing::{span, Level};
    use tracing_test::traced_test;

    #[test]
    fn test_terraform_managed_errors() {
        let could_not_load_plugin = r#"
Error:    Could not load plugin

   
Plugin reinitialization required. Please run "terraform init".

Plugins are external binaries that Terraform uses to access and manipulate
resources. The configuration provided requires plugins which can't be located,
don't satisfy the version constraints, or are otherwise incompatible.

Terraform automatically discovers provider requirements from your
configuration, including providers used in child modules. To see the
requirements and constraints, run "terraform providers".

Failed to instantiate provider "registry.terraform.io/hashicorp/time" to
obtain schema: the cached package for registry.terraform.io/hashicorp/time
0.7.2 (in .terraform/providers) does not match any of the checksums recorded
in the dependency lock file
        "#;

        let could_not_load_plugin_error = SimpleError {
            kind: SimpleErrorKind::Other,
            message: Some(could_not_load_plugin.to_string()),
        };
        assert!(manage_common_issues(
            &"/tmp/do_not_exists".to_string(),
            &could_not_load_plugin_error
        ));
    }

    #[test]
    #[traced_test]
    // https://github.com/hashicorp/terraform/issues/28041
    fn test_terraform_init_lock_issue() {
        let span = span!(Level::TRACE, "terraform_test");
        let _enter = span.enter();

        // those 2 files are a voluntary broken config, it should detect it and auto repair
        let terraform_lock_file = r#"
# This file is maintained automatically by "terraform init".
# Manual edits may be lost in future updates.

provider "registry.terraform.io/hashicorp/local" {
  version     = "1.4.0"
  constraints = "~> 1.4"
  hashes = [
    "h1:bZN53L85E49Pc5o3HUUCUqP5rZBziMF2KfKOaFsqN7w=",
    "zh:1b265fcfdce8cc3ccb51969c6d7a61531bf8a6e1218d95c1a74c40f25595c74b",
  ]
}
        "#;

        let provider_file = r#"
terraform {
  required_providers {
    local = {
      source = "hashicorp/local"
      version = "~> 1.4"
    }
  }
  required_version = ">= 0.14"
}
        "#;

        let dest_dir = "/tmp/test";
        let _ = fs::create_dir_all(&dest_dir).unwrap();

        let _ = fs::write(format!("{}/.terraform.lock.hcl", &dest_dir), terraform_lock_file);
        let _ = fs::write(format!("{}/providers.tf", &dest_dir), provider_file);

        let res = terraform_init_validate(dest_dir);

        assert!(res.is_ok());
    }
}

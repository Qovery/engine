use bitflags::bitflags;
use dirs::home_dir;
use retry::delay::Fixed;
use retry::OperationResult;

use crate::cmd::command::QoveryCommand;
use crate::constants::TF_PLUGIN_CACHE_DIR;
use rand::Rng;
use retry::Error::Operation;
use std::fmt::{Display, Formatter};
use std::{env, fs, thread, time};

bitflags! {
    /// Using a bitwise operator here allows to combine actions
    struct TerraformAction: u32 {
        const INIT = 0b00000001;
        const VALIDATE = 0b00000010;
        const PLAN = 0b00000100;
        const APPLY = 0b00001000;
        const DESTROY = 0b00010000;
        const STATE_LIST = 0b00100000;
    }
}

pub enum TerraformError {
    Unknown {
        /// message: Safe message.
        message: String,
        /// raw_message: raw Terraform error message with all details.
        raw_message: String,
    },
    ConfigFileNotFound {
        path: String,
        /// raw_message: raw Terraform error message with all details.
        raw_message: String,
    },
    ConfigFileInvalidContent {
        path: String,
        /// raw_message: raw Terraform error message with all details.
        raw_message: String,
    },
    CannotDeleteLockFile {
        terraform_provider_lock: String,
        /// raw_message: raw Terraform error message with all details.
        raw_message: String,
    },
    CannotRemoveEntryOutOfStateList {
        entry_to_be_removed: String,
        /// raw_message: raw Terraform error message with all details.
        raw_message: String,
    },
    ContextUnsupportedParameterValue {
        service_type: String,
        parameter_name: String,
        parameter_value: String,
        /// raw_message: raw Terraform error message with all details.
        raw_message: String,
    },
    Initialize {
        /// raw_message: raw Terraform error message with all details.
        raw_message: String,
    },
    Validate {
        /// raw_message: raw Terraform error message with all details.
        raw_message: String,
    },
    StateList {
        /// raw_message: raw Terraform error message with all details.
        raw_message: String,
    },
    Plan {
        /// raw_message: raw Terraform error message with all details.
        raw_message: String,
    },
    Apply {
        /// raw_message: raw Terraform error message with all details.
        raw_message: String,
    },
    Destroy {
        /// raw_message: raw Terraform error message with all details.
        raw_message: String,
    },
}

impl Display for TerraformError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let message: String = match self {
            TerraformError::Unknown { message, raw_message } => format!("{}\n{}", message, raw_message),
            TerraformError::CannotDeleteLockFile {
                terraform_provider_lock,
                raw_message,
            } => format!(
                "Wasn't able to delete terraform lock file {}\n{}",
                terraform_provider_lock, raw_message
            ),
            TerraformError::Initialize { raw_message } => {
                format!("Error while performing Terraform init\n{}", raw_message,)
            }
            TerraformError::Validate { raw_message } => {
                format!("Error while performing Terraform validate\n{}", raw_message,)
            }
            TerraformError::StateList { raw_message } => {
                format!("Error while performing Terraform statelist\n{}", raw_message,)
            }
            TerraformError::Plan { raw_message } => {
                format!("Error while performing Terraform plan\n{}", raw_message,)
            }
            TerraformError::Apply { raw_message } => {
                format!("Error while performing Terraform apply\n{}", raw_message,)
            }
            TerraformError::Destroy { raw_message } => {
                format!("Error while performing Terraform destroy\n{}", raw_message,)
            }
            TerraformError::ConfigFileNotFound { path, raw_message } => {
                format!(
                    "Error while trying to get Terraform configuration file `{}`. \n{}",
                    path, raw_message,
                )
            }
            TerraformError::ConfigFileInvalidContent { path, raw_message } => {
                format!(
                    "Error while trying to read Terraform configuration file, content is invalid `{}`.\n{}",
                    path, raw_message,
                )
            }
            TerraformError::CannotRemoveEntryOutOfStateList {
                entry_to_be_removed,
                raw_message,
            } => {
                format!(
                    "Error while trying to remove entry `{}` from state list.\n{}",
                    entry_to_be_removed, raw_message,
                )
            }
            TerraformError::ContextUnsupportedParameterValue {
                service_type,
                parameter_name,
                parameter_value,
                raw_message,
            } => {
                format!(
                    "{} value `{}` not supported for parameter `{}`.\n{}",
                    service_type, parameter_value, parameter_name, raw_message,
                )
            }
        };

        f.write_str(&message)
    }
}

fn manage_common_issues(terraform_provider_lock: &str, err: &TerraformError) -> Result<(), TerraformError> {
    // Error: Failed to install provider from shared cache
    // in order to avoid lock errors on parallel run, let's sleep a bit
    // https://github.com/hashicorp/terraform/issues/28041

    let error_string = err.to_string();

    if error_string.contains("Failed to install provider from shared cache")
        || error_string.contains("Failed to install provider")
    {
        let sleep_time_int = rand::thread_rng().gen_range(20..45);
        let sleep_time = time::Duration::from_secs(sleep_time_int);

        // failed to install provider from shared cache, cleaning and sleeping before retrying...",
        thread::sleep(sleep_time);

        return match fs::remove_file(&terraform_provider_lock) {
            Ok(_) => Ok(()),
            Err(e) => Err(TerraformError::CannotDeleteLockFile {
                terraform_provider_lock: terraform_provider_lock.to_string(),
                raw_message: e.to_string(),
            }),
        };
    } else if error_string.contains("Plugin reinitialization required") {
        // terraform init is required
        return Ok(());
    }

    Err(TerraformError::Unknown {
        message: "Unknown Terraform error, no workaround to solve this issue automatically".to_string(),
        raw_message: error_string,
    })
}

fn terraform_init(root_dir: &str) -> Result<Vec<String>, TerraformError> {
    let terraform_provider_lock = format!("{}/.terraform.lock.hcl", &root_dir);

    let result = retry::retry(Fixed::from_millis(3000).take(5), || {
        // terraform init
        match terraform_exec(root_dir, vec!["init", "-no-color"]) {
            Ok(output) => OperationResult::Ok(output),
            Err(err) => {
                let _ = manage_common_issues(&terraform_provider_lock, &err);
                // Error while trying to run terraform init, retrying...
                OperationResult::Retry(err)
            }
        }
    });

    match result {
        Ok(output) => Ok(output),
        Err(Operation { error, .. }) => Err(error),
        Err(retry::Error::Internal(e)) => Err(TerraformError::Initialize { raw_message: e }),
    }
}

fn terraform_validate(root_dir: &str) -> Result<Vec<String>, TerraformError> {
    let terraform_provider_lock = format!("{}/.terraform.lock.hcl", &root_dir);

    let result = retry::retry(Fixed::from_millis(3000).take(5), || {
        // validate config
        match terraform_exec(root_dir, vec!["validate", "-no-color"]) {
            Ok(output) => OperationResult::Ok(output),
            Err(err) => {
                let _ = manage_common_issues(&terraform_provider_lock, &err);
                // error while trying to Terraform validate on the rendered templates
                OperationResult::Retry(err)
            }
        }
    });

    match result {
        Ok(output) => Ok(output),
        Err(Operation { error, .. }) => Err(error),
        Err(retry::Error::Internal(e)) => Err(TerraformError::Validate { raw_message: e }),
    }
}

pub fn terraform_state_list(root_dir: &str) -> Result<Vec<String>, TerraformError> {
    // get terraform state list output
    let result = retry::retry(Fixed::from_millis(3000).take(5), || {
        match terraform_exec(root_dir, vec!["state", "list"]) {
            Ok(out) => OperationResult::Ok(out),
            Err(err) => {
                // Error while trying to run terraform state list, retrying...
                OperationResult::Retry(err)
            }
        }
    });

    match result {
        Ok(output) => Ok(output),
        Err(Operation { error, .. }) => Err(error),
        Err(retry::Error::Internal(e)) => Err(TerraformError::StateList { raw_message: e }),
    }
}

pub fn terraform_plan(root_dir: &str) -> Result<Vec<String>, TerraformError> {
    // plan
    let result = retry::retry(Fixed::from_millis(3000).take(3), || {
        match terraform_exec(root_dir, vec!["plan", "-no-color", "-out", "tf_plan"]) {
            Ok(out) => OperationResult::Ok(out),
            Err(err) => {
                // Error while trying to Terraform plan the rendered templates
                OperationResult::Retry(err)
            }
        }
    });

    match result {
        Ok(output) => Ok(output),
        Err(Operation { error, .. }) => Err(error),
        Err(retry::Error::Internal(e)) => Err(TerraformError::Apply { raw_message: e }),
    }
}

fn terraform_apply(root_dir: &str) -> Result<Vec<String>, TerraformError> {
    let result = retry::retry(Fixed::from_millis(3000).take(5), || {
        // apply
        match terraform_exec(root_dir, vec!["apply", "-no-color", "-auto-approve", "tf_plan"]) {
            Ok(out) => OperationResult::Ok(out),
            Err(err) => {
                // Error while trying to run terraform apply on rendered templates, retrying...
                OperationResult::Retry(err)
            }
        }
    });

    match result {
        Ok(output) => Ok(output),
        Err(Operation { error, .. }) => Err(error),
        Err(retry::Error::Internal(e)) => Err(TerraformError::Apply { raw_message: e }),
    }
}

pub fn terraform_apply_with_tf_workers_resources(
    root_dir: &str,
    tf_workers_resources: Vec<String>,
) -> Result<Vec<String>, TerraformError> {
    let mut terraform_args_string = vec!["apply".to_string(), "-auto-approve".to_string()];
    for x in tf_workers_resources {
        terraform_args_string.push(format!("-target={}", x));
    }

    let result = retry::retry(Fixed::from_millis(3000).take(5), || {
        // apply
        match terraform_exec(root_dir, terraform_args_string.iter().map(|x| &**x).collect()) {
            Ok(out) => OperationResult::Ok(out),
            Err(err) => {
                // Error while trying to run terraform apply on rendered templates, retrying...
                OperationResult::Retry(err)
            }
        }
    });

    match result {
        Ok(output) => Ok(output),
        Err(Operation { error, .. }) => Err(error),
        Err(retry::Error::Internal(e)) => Err(TerraformError::Apply { raw_message: e }),
    }
}

pub fn terraform_state_rm_entry(root_dir: &str, entry: &str) -> Result<Vec<String>, TerraformError> {
    match terraform_exec(root_dir, vec!["state", "rm", entry]) {
        Ok(out) => Ok(out),
        Err(err) => {
            // Error while trying to run terraform state rm entry, retrying...
            Err(TerraformError::CannotRemoveEntryOutOfStateList {
                entry_to_be_removed: entry.to_string(),
                raw_message: err.to_string(),
            })
        }
    }
}

pub fn terraform_destroy(root_dir: &str) -> Result<Vec<String>, TerraformError> {
    // terraform destroy
    let result = retry::retry(Fixed::from_millis(3000).take(5), || {
        match terraform_exec(root_dir, vec!["destroy", "-no-color", "-auto-approve"]) {
            Ok(out) => OperationResult::Ok(out),
            Err(err) => {
                // Error while trying to run terraform destroy on rendered templates, retrying...
                OperationResult::Retry(err)
            }
        }
    });

    match result {
        Ok(output) => Ok(output),
        Err(Operation { error, .. }) => Err(error),
        Err(retry::Error::Internal(e)) => Err(TerraformError::Destroy { raw_message: e }),
    }
}

fn terraform_run(actions: TerraformAction, root_dir: &str, dry_run: bool) -> Result<Vec<String>, TerraformError> {
    let mut output = vec![];

    if actions.contains(TerraformAction::INIT) {
        output.extend(terraform_init(root_dir)?);
    }

    if actions.contains(TerraformAction::VALIDATE) {
        output.extend(terraform_validate(root_dir)?);
    }

    if actions.contains(TerraformAction::STATE_LIST) {
        output.extend(terraform_state_list(root_dir)?);
    }

    if actions.contains(TerraformAction::PLAN) || dry_run {
        output.extend(terraform_plan(root_dir)?);
    }

    if actions.contains(TerraformAction::APPLY) && !dry_run {
        output.extend(terraform_apply(root_dir)?);
    }

    if actions.contains(TerraformAction::DESTROY) && !dry_run {
        output.extend(terraform_destroy(root_dir)?);
    }

    Ok(output)
}

pub fn terraform_init_validate_plan_apply(root_dir: &str, dry_run: bool) -> Result<Vec<String>, TerraformError> {
    // Terraform init, validate, plan and apply
    terraform_run(
        TerraformAction::INIT | TerraformAction::VALIDATE | TerraformAction::PLAN | TerraformAction::APPLY,
        root_dir,
        dry_run,
    )
}

pub fn terraform_init_validate(root_dir: &str) -> Result<Vec<String>, TerraformError> {
    // Terraform init & validate
    terraform_run(TerraformAction::INIT | TerraformAction::VALIDATE, root_dir, false)
}

pub fn terraform_init_validate_destroy(
    root_dir: &str,
    run_apply_before_destroy: bool,
) -> Result<Vec<String>, TerraformError> {
    let mut terraform_actions_to_be_performed = TerraformAction::INIT | TerraformAction::VALIDATE;

    // better to apply before destroy to ensure terraform destroy will delete on all resources
    if run_apply_before_destroy {
        terraform_actions_to_be_performed |= TerraformAction::PLAN;
        terraform_actions_to_be_performed |= TerraformAction::APPLY;
    }

    terraform_run(terraform_actions_to_be_performed | TerraformAction::DESTROY, root_dir, false)
}

pub fn terraform_init_validate_state_list(root_dir: &str) -> Result<Vec<String>, TerraformError> {
    // Terraform init, validate and statelist
    terraform_run(
        TerraformAction::INIT | TerraformAction::VALIDATE | TerraformAction::STATE_LIST,
        root_dir,
        false,
    )
}

/// This method should not be exposed to the outside world, it's internal magic.
fn terraform_exec(root_dir: &str, args: Vec<&str>) -> Result<Vec<String>, TerraformError> {
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
    let envs = &[(TF_PLUGIN_CACHE_DIR, tf_plugin_cache_dir_value.as_str())];
    let mut cmd = QoveryCommand::new("terraform", &args, envs);
    cmd.set_current_dir(root_dir);

    let result = cmd.exec_with_output(
        &mut |line| {
            info!("{}", line);
            stdout.push(line);
        },
        &mut |line| {
            error!("{}", line);
            stderr.push(line);
        },
    );

    stdout.extend(stderr.clone());

    match result {
        Ok(_) => Ok(stdout),
        Err(_) => Err(TerraformError::Unknown {
            message: "Error while performing Terraform command.".to_string(),
            raw_message: format!(
                "command: terraform {} failed\nSTDOUT:\n{}\n STDERR:\n{}",
                args.iter().map(|e| e.to_string()).collect::<Vec<String>>().join(" "),
                stdout.join(" "),
                stderr.join(" ")
            ),
        }),
    }
}

#[cfg(test)]
mod tests {
    use crate::cmd::terraform::{manage_common_issues, terraform_init_validate, TerraformError};
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

        let could_not_load_plugin_error = TerraformError::Unknown {
            message: "unknown error".to_string(),
            raw_message: could_not_load_plugin.to_string(),
        };
        assert!(manage_common_issues("/tmp/do_not_exists", &could_not_load_plugin_error).is_ok());
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
        fs::create_dir_all(&dest_dir).unwrap();

        let _ = fs::write(format!("{}/.terraform.lock.hcl", &dest_dir), terraform_lock_file);
        let _ = fs::write(format!("{}/providers.tf", &dest_dir), provider_file);

        let res = terraform_init_validate(dest_dir);

        assert!(res.is_ok());
    }
}

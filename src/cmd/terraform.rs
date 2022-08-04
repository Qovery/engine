use bitflags::bitflags;
use dirs::home_dir;
use retry::delay::Fixed;
use retry::OperationResult;

use crate::cmd::command::QoveryCommand;
use crate::constants::TF_PLUGIN_CACHE_DIR;
use rand::Rng;
use regex::Regex;
use retry::Error::Operation;
use std::fmt::{Display, Formatter};
use std::{env, fs, thread, time};

bitflags! {
    /// Using a bitwise operator here allows to combine actions
    struct TerraformAction: u32 {
        const INIT = 0b00000001;
        const VALIDATE = 0b00000010;
        // const PLAN = 0b00000100; Not needed, apply and destroy should call plan on their end
        const APPLY = 0b00001000;
        const DESTROY = 0b00010000;
        const STATE_LIST = 0b00100000;
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum QuotaExceededError {
    ResourceLimitExceeded {
        resource_type: String,
        max_resource_count: Option<u32>,
    },

    // Cloud provider specifics
    // TODO(benjaminch): variant below this comment might probably not live here on the long run.
    // There is some cloud providers specific errors and it would make more sense to delegate logic
    // identifying those errors (trait implementation) on cloud provider side next to their kubernetes implementation.
    ScwNewAccountNeedsValidation,
}

#[derive(Debug, PartialEq)]
pub enum TerraformError {
    Unknown {
        terraform_args: Vec<String>,
        /// raw_message: raw Terraform error message with all details.
        raw_message: String,
    },
    InvalidCredentials {
        /// raw_message: raw Terraform error message with all details.
        raw_message: String,
    },
    QuotasExceeded {
        sub_type: QuotaExceededError,
        /// raw_message: raw Terraform error message with all details.
        raw_message: String,
    },
    NotEnoughPermissions {
        resource_type_and_name: String,
        action: String,
        user: String,
        /// raw_message: raw Terraform error message with all details.
        raw_message: String,
    },
    ServiceNotActivatedOptInRequired {
        service_type: String,
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
}

impl TerraformError {
    fn new(terraform_args: Vec<String>, raw_terraform_output: String) -> Self {
        // TODO(benjaminch): this logic might probably not live here on the long run.
        // There is some cloud providers specific errors and it would make more sense to delegate logic
        // identifying those errors (trait implementation) on cloud provider side next to their kubernetes implementation.

        // Quotas issues
        // SCW
        if raw_terraform_output.contains("<Code>QuotaExceeded</Code>") {
            // SCW bucket quotas issues example:
            // Request ID: None Body: <?xml version='1.0' encoding='UTF-8'?>\n<Error><Code>QuotaExceeded</Code><Message>Quota exceeded. Please contact support to upgrade your quotas.</Message><RequestId>tx111117bad3a44d56bd120-0062d1515d</RequestId></Error>
            return TerraformError::QuotasExceeded {
                sub_type: QuotaExceededError::ScwNewAccountNeedsValidation,
                raw_message: raw_terraform_output,
            };
        }

        // AWS
        if let Ok(aws_quotas_exceeded_re) =
            Regex::new(r"You've reached your quota for maximum (?P<resource_type>[\w?\s]+) for this account")
        {
            if let Some(cap) = aws_quotas_exceeded_re.captures(raw_terraform_output.as_str()) {
                if let Some(resource_type) = cap.name("resource_type").map(|e| e.as_str()) {
                    return TerraformError::QuotasExceeded {
                        sub_type: QuotaExceededError::ResourceLimitExceeded {
                            resource_type: resource_type.to_string(),
                            max_resource_count: None,
                        },
                        raw_message: raw_terraform_output.to_string(),
                    };
                }
            }
        }
        if let Ok(aws_service_not_activated_re) = Regex::new(
            r"Error fetching (?P<service_type>[\w?\s]+): OptInRequired: You are not subscribed to this service",
        ) {
            if let Some(cap) = aws_service_not_activated_re.captures(raw_terraform_output.as_str()) {
                if let Some(service_type) = cap.name("service_type").map(|e| e.as_str()) {
                    return TerraformError::ServiceNotActivatedOptInRequired {
                        service_type: service_type.to_string(),
                        raw_message: raw_terraform_output.to_string(),
                    };
                }
            }
        }
        if let Ok(aws_quotas_exceeded_re) = Regex::new(
            r"You have exceeded the limit of (?P<resource_type>[\w?\s]+) allowed on your AWS account \((?P<max_resource_count>\d+) by default\)",
        ) {
            if let Some(cap) = aws_quotas_exceeded_re.captures(raw_terraform_output.as_str()) {
                if let (Some(resource_type), Some(max_resource_count)) = (
                    cap.name("resource_type").map(|e| e.as_str()),
                    cap.name("max_resource_count")
                        .map(|e| e.as_str().parse::<u32>().unwrap_or(0)),
                ) {
                    return TerraformError::QuotasExceeded {
                        sub_type: QuotaExceededError::ResourceLimitExceeded {
                            resource_type: resource_type.to_string(),
                            max_resource_count: Some(max_resource_count),
                        },
                        raw_message: raw_terraform_output.to_string(),
                    };
                }
            }
        }
        if let Ok(aws_quotas_exceeded_re) = Regex::new(
            r"You have requested more (?P<resource_type>[\w?\s]+) capacity than your current [\w?\s]+ limit of (?P<max_resource_count>\d+)",
        ) {
            if let Some(cap) = aws_quotas_exceeded_re.captures(raw_terraform_output.as_str()) {
                if let (Some(resource_type), Some(max_resource_count)) = (
                    cap.name("resource_type").map(|e| e.as_str()),
                    cap.name("max_resource_count")
                        .map(|e| e.as_str().parse::<u32>().unwrap_or(0)),
                ) {
                    return TerraformError::QuotasExceeded {
                        sub_type: QuotaExceededError::ResourceLimitExceeded {
                            resource_type: resource_type.to_string(),
                            max_resource_count: Some(max_resource_count),
                        },
                        raw_message: raw_terraform_output.to_string(),
                    };
                }
            }
        }
        if let Ok(aws_quotas_exceeded_re) = Regex::new(
            r"Error creating (?P<resource_type>[\w?\s]+): \w+: The maximum number of [\w?\s]+ has been reached",
        ) {
            if let Some(cap) = aws_quotas_exceeded_re.captures(raw_terraform_output.as_str()) {
                if let Some(resource_type) = cap.name("resource_type").map(|e| e.as_str()) {
                    return TerraformError::QuotasExceeded {
                        sub_type: QuotaExceededError::ResourceLimitExceeded {
                            resource_type: resource_type.to_string(),
                            max_resource_count: None,
                        },
                        raw_message: raw_terraform_output.to_string(),
                    };
                }
            }
        }
        if raw_terraform_output.contains("error calling sts:GetCallerIdentity: operation error STS: GetCallerIdentity, https response error StatusCode: 403") {
            return TerraformError::InvalidCredentials {
                raw_message: raw_terraform_output,
            };
        }
        if raw_terraform_output.contains("error calling sts:GetCallerIdentity: operation error STS: GetCallerIdentity, https response error StatusCode: 403") {
            return TerraformError::InvalidCredentials {
                raw_message: raw_terraform_output,
            };
        }
        if let Ok(aws_not_enough_permissions_re) = Regex::new(
            r"AccessDenied: User: (?P<user>.+?) is not authorized to perform: (?P<action>.+?) on resource: (?P<resource_type_and_name>.+?) because",
        ) {
            if let Some(cap) = aws_not_enough_permissions_re.captures(raw_terraform_output.as_str()) {
                if let (Some(resource_type_and_name), Some(user), Some(action)) = (
                    cap.name("resource_type_and_name").map(|e| e.as_str()),
                    cap.name("user").map(|e| e.as_str()),
                    cap.name("action").map(|e| e.as_str()),
                ) {
                    return TerraformError::NotEnoughPermissions {
                        resource_type_and_name: resource_type_and_name.to_string(),
                        user: user.to_string(),
                        action: action.to_string(),
                        raw_message: raw_terraform_output.to_string(),
                    };
                }
            }
        }

        // This kind of error should be triggered as little as possible, ideally, there is no unknown errors
        // (un-catched) so we can act / report properly to the user.
        TerraformError::Unknown {
            terraform_args,
            raw_message: raw_terraform_output,
        }
    }

    /// Returns safe Terraform error message part (not full error message).
    pub fn to_safe_message(&self) -> String {
        match self {
            TerraformError::Unknown { terraform_args, .. } => format!(
                "Unknown error while performing Terraform command (`terraform {}`)",
                terraform_args.join(" "),
            ),
            TerraformError::InvalidCredentials { .. } => "Invalid credentials.".to_string(),
            TerraformError::NotEnoughPermissions {
                resource_type_and_name,
                user,
                action,
                ..
            } => format!(
                "Error, user `{}` cannot perform `{}` on `{}`.",
                user, action, resource_type_and_name
            ),
            TerraformError::CannotDeleteLockFile {
                terraform_provider_lock,
                ..
            } => format!("Wasn't able to delete terraform lock file `{}`.", terraform_provider_lock,),
            TerraformError::ConfigFileNotFound { path, .. } => {
                format!("Error while trying to get Terraform configuration file `{}`.", path,)
            }
            TerraformError::ConfigFileInvalidContent { path, .. } => {
                format!(
                    "Error while trying to read Terraform configuration file, content is invalid `{}`.",
                    path,
                )
            }
            TerraformError::CannotRemoveEntryOutOfStateList {
                entry_to_be_removed, ..
            } => {
                format!("Error while trying to remove entry `{}` from state list.", entry_to_be_removed,)
            }
            TerraformError::ContextUnsupportedParameterValue {
                service_type,
                parameter_name,
                parameter_value,
                ..
            } => {
                format!(
                    "Error {} value `{}` not supported for parameter `{}`.",
                    service_type, parameter_value, parameter_name,
                )
            }
            TerraformError::QuotasExceeded { sub_type, .. } => {
                format!(
                    "Error, cloud provider quotas exceeded. {}",
                    match sub_type {
                        QuotaExceededError::ScwNewAccountNeedsValidation =>
                            "SCW new account requires cloud provider validation.".to_string(),
                        QuotaExceededError::ResourceLimitExceeded {
                            resource_type,
                            max_resource_count,
                        } => format!(
                            "`{}` has reached its quotas{}.",
                            resource_type,
                            match max_resource_count {
                                None => "".to_string(),
                                Some(count) => format!(" of {}", count),
                            }
                        ),
                    },
                )
            }
            TerraformError::ServiceNotActivatedOptInRequired { service_type, .. } => {
                format!("Error, service `{}` requiring an opt-in is not activated.", service_type,)
            }
        }
    }
}

impl Display for TerraformError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let message: String = match self {
            TerraformError::Unknown { raw_message, .. } => {
                format!("{}, here is the error:\n{}", self.to_safe_message(), raw_message)
            }
            TerraformError::InvalidCredentials { raw_message } => {
                format!("{}\n{}", self.to_safe_message(), raw_message)
            }
            TerraformError::NotEnoughPermissions { raw_message, .. } => {
                format!("{}\n{}", self.to_safe_message(), raw_message)
            }
            TerraformError::CannotDeleteLockFile { raw_message, .. } => {
                format!("{}\n{}", self.to_safe_message(), raw_message)
            }
            TerraformError::ConfigFileNotFound { raw_message, .. } => {
                format!("{}\n{}", self.to_safe_message(), raw_message)
            }
            TerraformError::ConfigFileInvalidContent { raw_message, .. } => {
                format!("{}\n{}", self.to_safe_message(), raw_message)
            }
            TerraformError::CannotRemoveEntryOutOfStateList { raw_message, .. } => {
                format!("{}\n{}", self.to_safe_message(), raw_message)
            }
            TerraformError::ContextUnsupportedParameterValue { raw_message, .. } => {
                format!("{}\n{}", self.to_safe_message(), raw_message)
            }
            TerraformError::QuotasExceeded { raw_message, .. } => {
                format!("{}\n{}", self.to_safe_message(), raw_message)
            }
            TerraformError::ServiceNotActivatedOptInRequired { raw_message, .. } => {
                format!("{}\n{}", self.to_safe_message(), raw_message)
            }
        };

        f.write_str(&message)
    }
}

fn manage_common_issues(
    terraform_args: Vec<&str>,
    terraform_provider_lock: &str,
    err: &TerraformError,
) -> Result<(), TerraformError> {
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
        return match fs::remove_file(&terraform_provider_lock) {
            Ok(_) => {
                thread::sleep(sleep_time);
                Ok(())
            }
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
        terraform_args: terraform_args.iter().map(|e| e.to_string()).collect(),
        raw_message: error_string,
    })
}

fn terraform_init(root_dir: &str) -> Result<Vec<String>, TerraformError> {
    let terraform_args = vec!["init", "-no-color"];
    let terraform_provider_lock = format!("{}/.terraform.lock.hcl", &root_dir);

    let result = retry::retry(Fixed::from_millis(3000).take(5), || {
        // terraform init
        match terraform_exec(root_dir, terraform_args.clone()) {
            Ok(output) => OperationResult::Ok(output),
            Err(err) => {
                let _ = manage_common_issues(terraform_args.clone(), &terraform_provider_lock, &err);
                // Error while trying to run terraform init, retrying...
                OperationResult::Retry(err)
            }
        }
    });

    match result {
        Ok(output) => Ok(output),
        Err(Operation { error, .. }) => Err(error),
        Err(retry::Error::Internal(e)) => {
            Err(TerraformError::new(terraform_args.iter().map(|e| e.to_string()).collect(), e))
        }
    }
}

fn terraform_validate(root_dir: &str) -> Result<Vec<String>, TerraformError> {
    let terraform_args = vec!["validate", "-no-color"];
    let terraform_provider_lock = format!("{}/.terraform.lock.hcl", &root_dir);

    // Retry is not needed, fixing it to 1 only for the time being
    let result = retry::retry(Fixed::from_millis(3000).take(1), || {
        // validate config
        match terraform_exec(root_dir, terraform_args.clone()) {
            Ok(output) => OperationResult::Ok(output),
            Err(err) => {
                if manage_common_issues(terraform_args.clone(), &terraform_provider_lock, &err).is_ok() {
                    let _ = terraform_init(root_dir);
                };
                // error while trying to Terraform validate on the rendered templates
                OperationResult::Retry(err)
            }
        }
    });

    match result {
        Ok(output) => Ok(output),
        Err(Operation { error, .. }) => Err(error),
        Err(retry::Error::Internal(e)) => {
            Err(TerraformError::new(terraform_args.iter().map(|e| e.to_string()).collect(), e))
        }
    }
}

pub fn terraform_state_list(root_dir: &str) -> Result<Vec<String>, TerraformError> {
    // get terraform state list output
    let terraform_args = vec!["state", "list"];
    let result = retry::retry(Fixed::from_millis(3000).take(5), || {
        match terraform_exec(root_dir, terraform_args.clone()) {
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
        Err(retry::Error::Internal(e)) => {
            Err(TerraformError::new(terraform_args.iter().map(|e| e.to_string()).collect(), e))
        }
    }
}

pub fn terraform_plan(root_dir: &str) -> Result<Vec<String>, TerraformError> {
    // plan
    let terraform_args = vec!["plan", "-no-color", "-out", "tf_plan"];
    // Retry is not needed, fixing it to 1 only for the time being
    let result = retry::retry(Fixed::from_millis(3000).take(1), || {
        match terraform_exec(root_dir, terraform_args.clone()) {
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
        Err(retry::Error::Internal(e)) => {
            Err(TerraformError::new(terraform_args.iter().map(|e| e.to_string()).collect(), e))
        }
    }
}

fn terraform_apply(root_dir: &str) -> Result<Vec<String>, TerraformError> {
    let terraform_args = vec!["apply", "-no-color", "-auto-approve", "tf_plan"];
    // Retry is not needed, fixing it to 1 only for the time being
    let result = retry::retry(Fixed::from_millis(3000).take(1), || {
        // terraform plan first
        if let Err(err) = terraform_plan(root_dir) {
            return OperationResult::Retry(err);
        }

        // terraform apply
        match terraform_exec(root_dir, terraform_args.clone()) {
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
        Err(retry::Error::Internal(e)) => {
            Err(TerraformError::new(terraform_args.iter().map(|e| e.to_string()).collect(), e))
        }
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
        // terraform plan first
        if let Err(err) = terraform_plan(root_dir) {
            return OperationResult::Retry(err);
        }

        // terraform apply
        match terraform_exec(root_dir, terraform_args_string.iter().map(|e| e.as_str()).collect()) {
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
        Err(retry::Error::Internal(e)) => Err(TerraformError::new(
            terraform_args_string.iter().map(|e| e.to_string()).collect(),
            e,
        )),
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
    let terraform_args = vec!["destroy", "-no-color", "-auto-approve"];
    let result = retry::retry(Fixed::from_millis(3000).take(5), || {
        // terraform plan first
        if let Err(err) = terraform_plan(root_dir) {
            return OperationResult::Retry(err);
        }

        // terraform destroy
        match terraform_exec(root_dir, terraform_args.clone()) {
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
        Err(retry::Error::Internal(e)) => {
            Err(TerraformError::new(terraform_args.iter().map(|e| e.to_string()).collect(), e))
        }
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
        TerraformAction::INIT | TerraformAction::VALIDATE | TerraformAction::APPLY,
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
        Err(_) => Err(TerraformError::new(
            args.iter().map(|e| e.to_string()).collect(),
            stderr.join("\n"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use crate::cmd::terraform::{manage_common_issues, terraform_init_validate, QuotaExceededError, TerraformError};
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

        let terraform_args = vec!["apply"];

        let could_not_load_plugin_error = TerraformError::Unknown {
            terraform_args: terraform_args.iter().map(|e| e.to_string()).collect(),
            raw_message: could_not_load_plugin.to_string(),
        };
        assert!(manage_common_issues(terraform_args, "/tmp/do_not_exists", &could_not_load_plugin_error).is_ok());
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

    #[test]
    fn test_terraform_error_scw_quotas_issue() {
        // setup:
        let raw_message = "Request ID: None Body: <?xml version='1.0' encoding='UTF-8'?>\n<Error><Code>QuotaExceeded</Code><Message>Quota exceeded. Please contact support to upgrade your quotas.</Message><RequestId>tx111117bad3a44d56bd120-0062d1515d</RequestId></Error>".to_string();

        // execute:
        let result = TerraformError::new(vec!["apply".to_string()], raw_message.to_string());

        // validate:
        assert_eq!(
            TerraformError::QuotasExceeded {
                sub_type: QuotaExceededError::ScwNewAccountNeedsValidation,
                raw_message
            },
            result
        );
    }

    #[test]
    fn test_terraform_error_aws_quotas_issue() {
        // setup:
        struct TestCase<'a> {
            input_raw_message: &'a str,
            expected_terraform_error: TerraformError,
        }

        let test_cases = vec![
            TestCase {
                input_raw_message: "You have exceeded the limit of vCPUs allowed on your AWS account (32 by default).",
                expected_terraform_error: TerraformError::QuotasExceeded {
                    sub_type: QuotaExceededError::ResourceLimitExceeded {
                        resource_type: "vCPUs".to_string(),
                        max_resource_count: Some(32),
                    },
                    raw_message: "You have exceeded the limit of vCPUs allowed on your AWS account (32 by default)."
                        .to_string(),
                },
            },
            TestCase {
                input_raw_message:
                    "Error creating EIP: AddressLimitExceeded: The maximum number of addresses has been reached.",
                expected_terraform_error: TerraformError::QuotasExceeded {
                    sub_type: QuotaExceededError::ResourceLimitExceeded {
                        resource_type: "EIP".to_string(),
                        max_resource_count: None,
                    },
                    raw_message:
                        "Error creating EIP: AddressLimitExceeded: The maximum number of addresses has been reached."
                            .to_string(),
                },
            },
            TestCase {
                input_raw_message:
                    "Error: error configuring Terraform AWS Provider: error validating provider credentials: error calling sts:GetCallerIdentity: operation error STS: GetCallerIdentity, https response error StatusCode: 403",
                expected_terraform_error: TerraformError::InvalidCredentials {
                    raw_message:
                        "Error: error configuring Terraform AWS Provider: error validating provider credentials: error calling sts:GetCallerIdentity: operation error STS: GetCallerIdentity, https response error StatusCode: 403"
                            .to_string(),
                },
            },
            TestCase {
                input_raw_message: "Error creating VPC: VpcLimitExceeded: The maximum number of VPCs has been reached.",
                expected_terraform_error: TerraformError::QuotasExceeded {
                    sub_type: QuotaExceededError::ResourceLimitExceeded {
                        resource_type: "VPC".to_string(),
                        max_resource_count: None,
                    },
                    raw_message: "Error creating VPC: VpcLimitExceeded: The maximum number of VPCs has been reached."
                        .to_string(),
                },
            },
            TestCase {
                input_raw_message: "AsgInstanceLaunchFailures: Could not launch On-Demand Instances. VcpuLimitExceeded - You have requested more vCPU capacity than your current vCPU limit of 32 allows for the instance bucket that the specified instance type belongs to. Please visit http://aws.amazon.com/contact-us/ec2-request to request an adjustment to this limit. Launching EC2 instance failed.",
                expected_terraform_error: TerraformError::QuotasExceeded {
                    sub_type: QuotaExceededError::ResourceLimitExceeded {
                        resource_type: "vCPU".to_string(),
                        max_resource_count: Some(32),
                    },
                    raw_message: "AsgInstanceLaunchFailures: Could not launch On-Demand Instances. VcpuLimitExceeded - You have requested more vCPU capacity than your current vCPU limit of 32 allows for the instance bucket that the specified instance type belongs to. Please visit http://aws.amazon.com/contact-us/ec2-request to request an adjustment to this limit. Launching EC2 instance failed.".to_string(),
                },
            },
            TestCase {
                input_raw_message: "Releasing state lock. This may take a few moments...  Error: Error fetching Availability Zones: OptInRequired: You are not subscribed to this service. Please go to http://aws.amazon.com to subscribe. \tstatus code: 401, request id: e34e6aa4-bb37-44fe-a68c-b4859f3f6de9",
                expected_terraform_error: TerraformError::ServiceNotActivatedOptInRequired {
                    raw_message: "Releasing state lock. This may take a few moments...  Error: Error fetching Availability Zones: OptInRequired: You are not subscribed to this service. Please go to http://aws.amazon.com to subscribe. \tstatus code: 401, request id: e34e6aa4-bb37-44fe-a68c-b4859f3f6de9".to_string(),
                    service_type: "Availability Zones".to_string(),
                },
            },
            TestCase {
                input_raw_message: "AsgInstanceLaunchFailures: You've reached your quota for maximum Fleet Requests for this account. Launching EC2 instance failed.",
                expected_terraform_error: TerraformError::QuotasExceeded {
                    sub_type: QuotaExceededError::ResourceLimitExceeded {
                        resource_type: "Fleet Requests".to_string(),
                        max_resource_count: None,
                    },
                    raw_message: "AsgInstanceLaunchFailures: You've reached your quota for maximum Fleet Requests for this account. Launching EC2 instance failed.".to_string(),
                },
            },
        ];

        for tc in test_cases {
            // execute:
            let result = TerraformError::new(vec!["apply".to_string()], tc.input_raw_message.to_string());

            // validate:
            assert_eq!(tc.expected_terraform_error, result);
        }
    }

    #[test]
    fn test_terraform_error_aws_permissions_issue() {
        // setup:
        let raw_message = "Error: error creating IAM policy qovery-aws-EBS-CSI-Driver-z2242cca3: AccessDenied: User: arn:aws:iam::542561660426:user/thomas is not authorized to perform: iam:CreatePolicy on resource: policy qovery-aws-EBS-CSI-Driver-z2242cca3 because no identity-based policy allows the iam:CreatePolicy action status code: 403, request id: 01ca1501-a0db-438e-a6db-4a2628236cba".to_string();

        // execute:
        let result = TerraformError::new(vec!["apply".to_string()], raw_message.to_string());

        // validate:
        assert_eq!(
            TerraformError::NotEnoughPermissions {
                user: "arn:aws:iam::542561660426:user/thomas".to_string(),
                action: "iam:CreatePolicy".to_string(),
                resource_type_and_name: "policy qovery-aws-EBS-CSI-Driver-z2242cca3".to_string(),
                raw_message,
            },
            result
        );
    }
}

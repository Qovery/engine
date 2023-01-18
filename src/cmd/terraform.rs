use bitflags::bitflags;
use dirs::home_dir;
use retry::delay::Fixed;
use retry::OperationResult;

use crate::cmd::command::{ExecutableCommand, QoveryCommand};
use crate::constants::TF_PLUGIN_CACHE_DIR;
use crate::events::{EngineEvent, EventDetails, EventMessage};
use crate::logger::Logger;
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
        const MIGRATE_CLOUDWATCH = 0b01000000;
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
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

#[derive(Debug, PartialEq, Eq, Clone)]
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
    AccountBlockedByProvider {
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
    AlreadyExistingResource {
        resource_type: String,
        /// raw_message: raw Terraform error message with all details.
        raw_message: String,
    },
    WaitingTimeoutResource {
        resource_type: String,
        resource_identifier: String,
        /// raw_message: raw Terraform error message with all details.
        raw_message: String,
    },
    WrongExpectedState {
        resource_kind: String,
        resource_name: String,
        /// raw_message: raw Terraform error message with all details.
        raw_message: String,
    },
    ResourceDependencyViolation {
        resource_kind: String,
        resource_name: String,
        /// raw_message: raw Terraform error message with all details.
        raw_message: String,
    },
    InstanceTypeDoesntExist {
        /// Providers doesn't always provide back with instance type requested ... so might be None
        instance_type: Option<String>,
        /// raw_message: raw Terraform error message with all details.
        raw_message: String,
    },
    InstanceVolumeCannotBeDownSized {
        instance_id: String,
        volume_id: String,
        /// raw_message: raw Terraform error message with all details.
        raw_message: String,
    },
    InvalidCIDRBlock {
        cidr: String,
        /// raw_message: raw Terraform error message with all details.
        raw_message: String,
    },
    MultipleInterruptsReceived {
        /// raw_message: raw Terraform error message with all details.
        raw_message: String,
    },
    StateLocked {
        lock_id: String,
        /// raw_message: raw Terraform error message with all details.
        raw_message: String,
    },
}

impl TerraformError {
    fn new(terraform_args: Vec<String>, raw_terraform_std_output: String, raw_terraform_error_output: String) -> Self {
        // TODO(benjaminch): this logic might probably not live here on the long run.
        // There is some cloud providers specific errors and it would make more sense to delegate logic
        // identifying those errors (trait implementation) on cloud provider side next to their kubernetes implementation.

        // Cloud account issue
        // AWS
        if raw_terraform_error_output.contains("Blocked: This account is currently blocked") {
            return TerraformError::AccountBlockedByProvider {
                raw_message: raw_terraform_error_output,
            };
        }

        // Quotas issues
        // SCW
        if raw_terraform_error_output.contains("<Code>QuotaExceeded</Code>") {
            // SCW bucket quotas issues example:
            // Request ID: None Body: <?xml version='1.0' encoding='UTF-8'?>\n<Error><Code>QuotaExceeded</Code><Message>Quota exceeded. Please contact support to upgrade your quotas.</Message><RequestId>tx111117bad3a44d56bd120-0062d1515d</RequestId></Error>
            return TerraformError::QuotasExceeded {
                sub_type: QuotaExceededError::ScwNewAccountNeedsValidation,
                raw_message: raw_terraform_error_output,
            };
        }

        // AWS
        if let Ok(aws_quotas_exceeded_re) =
            Regex::new(r"You've reached your quota for maximum (?P<resource_type>[\w?\s]+) for this account")
        {
            if let Some(cap) = aws_quotas_exceeded_re.captures(raw_terraform_error_output.as_str()) {
                if let Some(resource_type) = cap.name("resource_type").map(|e| e.as_str()) {
                    return TerraformError::QuotasExceeded {
                        sub_type: QuotaExceededError::ResourceLimitExceeded {
                            resource_type: resource_type.to_string(),
                            max_resource_count: None,
                        },
                        raw_message: raw_terraform_error_output.to_string(),
                    };
                }
            }
        }
        if let Ok(aws_service_not_activated_re) = Regex::new(
            r"Error fetching (?P<service_type>[\w?\s]+): OptInRequired: You are not subscribed to this service",
        ) {
            if let Some(cap) = aws_service_not_activated_re.captures(raw_terraform_error_output.as_str()) {
                if let Some(service_type) = cap.name("service_type").map(|e| e.as_str()) {
                    return TerraformError::ServiceNotActivatedOptInRequired {
                        service_type: service_type.to_string(),
                        raw_message: raw_terraform_error_output.to_string(),
                    };
                }
            }
        }
        if let Ok(aws_service_not_activated_re) = Regex::new(
            r"Error creating (?P<service_type>[\w?\s]+): OptInRequired: You are not subscribed to this service",
        ) {
            if let Some(cap) = aws_service_not_activated_re.captures(raw_terraform_error_output.as_str()) {
                if let Some(service_type) = cap.name("service_type").map(|e| e.as_str()) {
                    return TerraformError::ServiceNotActivatedOptInRequired {
                        service_type: service_type.to_string(),
                        raw_message: raw_terraform_error_output.to_string(),
                    };
                }
            }
        }
        if let Ok(aws_quotas_exceeded_re) = Regex::new(
            r"You have exceeded the limit of (?P<resource_type>[\w?\s]+) allowed on your AWS account \((?P<max_resource_count>\d+) by default\)",
        ) {
            if let Some(cap) = aws_quotas_exceeded_re.captures(raw_terraform_error_output.as_str()) {
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
                        raw_message: raw_terraform_error_output.to_string(),
                    };
                }
            }
        }
        if let Ok(aws_quotas_exceeded_re) = Regex::new(
            r"You have requested more (?P<resource_type>[\w?\s]+) capacity than your current [\w?\s]+ limit of (?P<max_resource_count>\d+)",
        ) {
            if let Some(cap) = aws_quotas_exceeded_re.captures(raw_terraform_error_output.as_str()) {
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
                        raw_message: raw_terraform_error_output.to_string(),
                    };
                }
            }
        }
        if let Ok(aws_quotas_exceeded_re) = Regex::new(
            r"Error creating (?P<resource_type>[\w?\s]+): \w+: The maximum number of [\w?\s]+ has been reached",
        ) {
            if let Some(cap) = aws_quotas_exceeded_re.captures(raw_terraform_error_output.as_str()) {
                if let Some(resource_type) = cap.name("resource_type").map(|e| e.as_str()) {
                    return TerraformError::QuotasExceeded {
                        sub_type: QuotaExceededError::ResourceLimitExceeded {
                            resource_type: resource_type.to_string(),
                            max_resource_count: None,
                        },
                        raw_message: raw_terraform_error_output.to_string(),
                    };
                }
            }
        }
        if let Ok(aws_quotas_exceeded_re) = Regex::new(
            r"error creating EC2 (?P<resource_type>[\w?\s]+): \w+: The maximum number of [\w?\s]+ has been reached",
        ) {
            if let Some(cap) = aws_quotas_exceeded_re.captures(raw_terraform_error_output.as_str()) {
                if let Some(resource_type) = cap.name("resource_type").map(|e| e.as_str()) {
                    return TerraformError::QuotasExceeded {
                        sub_type: QuotaExceededError::ResourceLimitExceeded {
                            resource_type: resource_type.to_string(),
                            max_resource_count: None,
                        },
                        raw_message: raw_terraform_error_output.to_string(),
                    };
                }
            }
        }
        if let Ok(aws_quotas_exceeded_re) = Regex::new(
            r"InvalidParameterException: Limit of (?P<max_resource_count>[\d]+) (?P<resource_type>[\w?\s]+) exceeded",
        ) {
            if let Some(cap) = aws_quotas_exceeded_re.captures(raw_terraform_error_output.as_str()) {
                if let (Some(max_resource_count), Some(resource_type)) = (
                    cap.name("max_resource_count")
                        .map(|e| e.as_str().parse::<u32>().unwrap_or(0)),
                    cap.name("resource_type").map(|e| e.as_str()),
                ) {
                    return TerraformError::QuotasExceeded {
                        sub_type: QuotaExceededError::ResourceLimitExceeded {
                            resource_type: resource_type.to_string(),
                            max_resource_count: Some(max_resource_count),
                        },
                        raw_message: raw_terraform_error_output.to_string(),
                    };
                }
            }
        }

        // State issue
        // AWS
        if let Ok(aws_state_expected_re) = Regex::new(
            r"Error modifying (?P<resource_kind>\w+) instance (?P<resource_name>.+?): \w+: You can't modify a .+",
        ) {
            if let Some(cap) = aws_state_expected_re.captures(raw_terraform_error_output.as_str()) {
                if let (Some(resource_kind), Some(resource_name)) = (
                    cap.name("resource_kind").map(|e| e.as_str()),
                    cap.name("resource_name").map(|e| e.as_str()),
                ) {
                    return TerraformError::WrongExpectedState {
                        resource_name: resource_name.to_string(),
                        resource_kind: resource_kind.to_string(),
                        raw_message: raw_terraform_error_output.to_string(),
                    };
                }
            }
        }

        // Dependencies issues
        // AWS
        if let Ok(aws_state_expected_re) = Regex::new(
            r"Error:? deleting (?P<resource_kind>.+?)( \(.+?\))?: DependencyViolation: .+ '(?P<resource_name>.+?)' has dependencies and cannot be deleted",
        ) {
            if let Some(cap) = aws_state_expected_re.captures(raw_terraform_error_output.as_str()) {
                if let (Some(resource_kind), Some(resource_name)) = (
                    cap.name("resource_kind").map(|e| e.as_str()),
                    cap.name("resource_name").map(|e| e.as_str()),
                ) {
                    return TerraformError::ResourceDependencyViolation {
                        resource_name: resource_name.to_string(),
                        resource_kind: resource_kind.to_string(),
                        raw_message: raw_terraform_error_output.to_string(),
                    };
                }
            }
        }

        // Invalid credentials issues
        //SCW
        if raw_terraform_error_output.contains("error calling sts:GetCallerIdentity: operation error STS: GetCallerIdentity, https response error StatusCode: 403") {
            return TerraformError::InvalidCredentials {
                raw_message: raw_terraform_error_output,
            };
        }
        if raw_terraform_error_output.contains("error calling sts:GetCallerIdentity: operation error STS: GetCallerIdentity, https response error StatusCode: 403") {
            return TerraformError::InvalidCredentials {
                raw_message: raw_terraform_error_output,
            };
        }
        if let Ok(aws_not_enough_permissions_re) = Regex::new(
            r"AccessDenied: User: (?P<user>.+?) is not authorized to perform: (?P<action>.+?) on resource: (?P<resource_type_and_name>.+?) because",
        ) {
            if let Some(cap) = aws_not_enough_permissions_re.captures(raw_terraform_error_output.as_str()) {
                if let (Some(resource_type_and_name), Some(user), Some(action)) = (
                    cap.name("resource_type_and_name").map(|e| e.as_str()),
                    cap.name("user").map(|e| e.as_str()),
                    cap.name("action").map(|e| e.as_str()),
                ) {
                    return TerraformError::NotEnoughPermissions {
                        resource_type_and_name: resource_type_and_name.to_string(),
                        user: user.to_string(),
                        action: action.to_string(),
                        raw_message: raw_terraform_error_output.to_string(),
                    };
                }
            }
        }

        // Resources issues
        // AWS
        // InvalidParameterException: The following supplied instance types do not exist: [t3a.medium]
        if let Ok(aws_wrong_instance_type_re) = Regex::new(
            r"InvalidParameterException: The following supplied instance types do not exist: \[(?P<instance_type>.+?)\]",
        ) {
            if let Some(cap) = aws_wrong_instance_type_re.captures(raw_terraform_error_output.as_str()) {
                if let Some(instance_type) = cap.name("instance_type").map(|e| e.as_str()) {
                    return TerraformError::InstanceTypeDoesntExist {
                        instance_type: Some(instance_type.to_string()),
                        raw_message: raw_terraform_error_output.to_string(),
                    };
                }
            }
        }
        // InvalidParameterValue: Invalid value 'wrong-instance-type' for InstanceType
        if let Ok(aws_wrong_instance_type_re) =
            Regex::new(r"InvalidParameterValue: Invalid value '(?P<instance_type>.+?)' for InstanceType")
        {
            if let Some(cap) = aws_wrong_instance_type_re.captures(raw_terraform_error_output.as_str()) {
                if let Some(instance_type) = cap.name("instance_type").map(|e| e.as_str()) {
                    return TerraformError::InstanceTypeDoesntExist {
                        instance_type: Some(instance_type.to_string()),
                        raw_message: raw_terraform_error_output.to_string(),
                    };
                }
            }
        }
        if raw_terraform_error_output.contains(
            "Error: creating EC2 Instance: Unsupported: The requested configuration is currently not supported",
        ) {
            // That's a shame but with the error message, AWS doesn't provide requested instance type, so cannot provide it back in error message.
            return TerraformError::InstanceTypeDoesntExist {
                instance_type: None,
                raw_message: raw_terraform_error_output,
            };
        }
        // InvalidParameterValue: New size cannot be smaller than existing size
        if let Ok(aws_wrong_instance_type_re) = Regex::new(
            r"Error: updating EC2 Instance \((?P<instance_id>.+?)\) volume \((?P<volume_id>.+?)\): InvalidParameterValue: New size cannot be smaller than existing size",
        ) {
            if let Some(cap) = aws_wrong_instance_type_re.captures(raw_terraform_error_output.as_str()) {
                if let (Some(instance_id), Some(volume_id)) = (
                    cap.name("instance_id").map(|e| e.as_str()),
                    cap.name("volume_id").map(|e| e.as_str()),
                ) {
                    return TerraformError::InstanceVolumeCannotBeDownSized {
                        instance_id: instance_id.to_string(),
                        volume_id: volume_id.to_string(),
                        raw_message: raw_terraform_error_output.to_string(),
                    };
                }
            }
        }
        // InvalidParameterValue: The destination CIDR block x.x.x.x/x is equal to or more specific than one of this VPC's CIDR blocks.
        if let Ok(aws_wrong_cidr) = Regex::new(
            r"InvalidParameterValue: The destination CIDR block \((?P<cidr>.+?)\) is equal to or more specific than one of this VPC's CIDR blocks. This route can target only an interface or an instance",
        ) {
            if let Some(cap) = aws_wrong_cidr.captures(raw_terraform_error_output.as_str()) {
                if let Some(wrong_cidr) = cap.name("cidr").map(|e| e.as_str()) {
                    return TerraformError::InvalidCIDRBlock {
                        cidr: wrong_cidr.to_string(),
                        raw_message: raw_terraform_error_output.to_string(),
                    };
                }
            }
        }

        // SCW
        if raw_terraform_error_output.contains("scaleway-sdk-go: waiting for")
            && raw_terraform_error_output.contains("failed: timeout after")
        {
            if let Ok(scw_resource_issue) = Regex::new(
                r"(?P<resource_type>\bscaleway_(?:\w*.\w*)): Refreshing state... \[id=(?P<resource_identifier>[\w\W\d]+)]",
            ) {
                if let Some(cap) = scw_resource_issue.captures(raw_terraform_std_output.as_str()) {
                    if let (Some(resource_type), Some(resource_identifier)) = (
                        cap.name("resource_type").map(|e| e.as_str()),
                        cap.name("resource_identifier").map(|e| e.as_str()),
                    ) {
                        return TerraformError::WaitingTimeoutResource {
                            resource_type: resource_type.to_string(),
                            resource_identifier: resource_identifier.to_string(),
                            raw_message: raw_terraform_error_output,
                        };
                    }
                }
            }
        }

        if raw_terraform_error_output.contains("scaleway-sdk-go: invalid argument(s):")
            && raw_terraform_error_output.contains("must be unique across the project")
        {
            if let Ok(scw_resource_issue) = Regex::new(r"(?P<resource_type>\bscaleway_(?:\w*.\w*)): Creating...") {
                if let Some(cap) = scw_resource_issue.captures(raw_terraform_std_output.as_str()) {
                    if let Some(resource_type) = cap.name("resource_type").map(|e| e.as_str()) {
                        return TerraformError::AlreadyExistingResource {
                            resource_type: resource_type.to_string(),
                            raw_message: raw_terraform_error_output,
                        };
                    }
                }
            }
        }

        // Terraform general errors
        if raw_terraform_error_output.contains("Two interrupts received. Exiting immediately.") {
            return TerraformError::MultipleInterruptsReceived {
                raw_message: raw_terraform_error_output,
            };
        }

        if raw_terraform_error_output.contains("Error acquiring the state lock")
            && raw_terraform_error_output.contains("Lock Info:")
        {
            if let Ok(tf_state_lock) = Regex::new(
                r"ID:\s+(?P<lock_id>\b(?:[0-9a-fA-F]{8}\-[0-9a-fA-F]{4}\-[0-9a-fA-F]{4}\-[0-9a-fA-F]{4}\-[0-9a-fA-F]{12}))",
            ) {
                if let Some(cap) = tf_state_lock.captures(raw_terraform_error_output.as_str()) {
                    if let Some(lock_id) = cap.name("lock_id").map(|e| e.as_str()) {
                        return TerraformError::StateLocked {
                            lock_id: lock_id.to_string(),
                            raw_message: raw_terraform_error_output,
                        };
                    }
                }
            }
        }

        // This kind of error should be triggered as little as possible, ideally, there is no unknown errors
        // (un-catched) so we can act / report properly to the user.
        TerraformError::Unknown {
            terraform_args,
            raw_message: raw_terraform_error_output,
        }
    }

    /// Returns safe Terraform error message part (not full error message).
    pub fn to_safe_message(&self) -> String {
        match self {
            TerraformError::Unknown { terraform_args, .. } => format!(
                "Unknown error while performing Terraform command (`terraform {}`)",
                terraform_args.join(" "),
            ),
            TerraformError::MultipleInterruptsReceived { .. } => "Multiple interrupts received, stopping immediately.".to_string(),
            TerraformError::AccountBlockedByProvider { .. } => "Yout account has been blocked by cloud provider.".to_string(),
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
            TerraformError::AlreadyExistingResource { resource_type, .. } => {
                format!("Error, resource {} already exists.", resource_type)
            }
            TerraformError::ResourceDependencyViolation { resource_name, resource_kind, .. } => {
                format!("Error, resource {} `{}` has dependency violation.", resource_kind, resource_name)
            }
            TerraformError::WaitingTimeoutResource {
                resource_type,
                resource_identifier,
                ..
            } => {
                format!("Error, waiting for resource {}:{} timeout.", resource_type, resource_identifier)
            }
            TerraformError::WrongExpectedState {
                resource_name: resource_type,
                resource_kind,
                raw_message,
            } => format!("Error, resource {}:{} was expected to be in another state. It happens when changes have been done Cloud provider side without Qovery. You need to fix it manually: {}", resource_type, resource_kind, raw_message),
            TerraformError::InstanceTypeDoesntExist { instance_type, ..} => format!("Error, requested instance type{} doesn't exist in cluster region.", match instance_type {
                Some(instance_type) => format!(" `{}`", instance_type),
                None => "".to_string(),
            }),
            TerraformError::InstanceVolumeCannotBeDownSized { instance_id, volume_id, .. } => {
                format!("Error, instance (`{}`) volume (`{}`) cannot be smaller than existing size.", instance_id, volume_id)
            },
            TerraformError::InvalidCIDRBlock {cidr,..} => {
                format!("Error, the CIDR block `{}` can't be used.", cidr)
            }
            TerraformError::StateLocked { lock_id, .. } => {
                format!("Error, terraform state is locked (lock_id: {})", lock_id)
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
            TerraformError::MultipleInterruptsReceived { raw_message, .. } => {
                format!("{}, here is the error:\n{}", self.to_safe_message(), raw_message)
            }
            TerraformError::AccountBlockedByProvider { raw_message, .. } => {
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
            TerraformError::AlreadyExistingResource { raw_message, .. } => {
                format!("{}\n{}", self.to_safe_message(), raw_message)
            }
            TerraformError::WaitingTimeoutResource { raw_message, .. } => {
                format!("{}\n{}", self.to_safe_message(), raw_message)
            }
            TerraformError::WrongExpectedState { raw_message, .. } => {
                format!("{}\n{}", self.to_safe_message(), raw_message)
            }
            TerraformError::ResourceDependencyViolation { raw_message, .. } => {
                format!("{}\n{}", self.to_safe_message(), raw_message)
            }
            TerraformError::InstanceTypeDoesntExist { raw_message, .. } => {
                format!("{}\n{}", self.to_safe_message(), raw_message)
            }
            TerraformError::InstanceVolumeCannotBeDownSized { raw_message, .. } => {
                format!("{}\n{}", self.to_safe_message(), raw_message)
            }
            TerraformError::InvalidCIDRBlock { raw_message, .. } => {
                format!("{}\n{}", self.to_safe_message(), raw_message)
            }
            TerraformError::StateLocked { raw_message, .. } => {
                format!("{}\n{}", self.to_safe_message(), raw_message)
            }
        };

        f.write_str(&message)
    }
}

fn manage_common_issues(
    root_dir: &str,
    terraform_provider_lock: &str,
    err: &TerraformError,
) -> Result<Vec<String>, TerraformError> {
    terraform_plugins_failed_load(root_dir, err, terraform_provider_lock)?;

    Ok(vec![])
}

fn terraform_plugins_failed_load(
    root_dir: &str,
    error: &TerraformError,
    terraform_provider_lock: &str,
) -> Result<Vec<String>, TerraformError> {
    // Error: Failed to install provider from shared cache
    // in order to avoid lock errors on parallel run, let's sleep a bit
    // https://github.com/hashicorp/terraform/issues/28041
    let error_string = error.to_string();
    let sleep_time_int = rand::thread_rng().gen_range(20..45);
    let sleep_time = time::Duration::from_secs(sleep_time_int);

    if error_string.contains("Failed to install provider from shared cache")
        || error_string.contains("Failed to install provider")
        || (error_string.contains("The specified plugin cache dir") && error_string.contains("cannot be opened"))
    {
        if let Err(e) = fs::remove_file(terraform_provider_lock) {
            return Err(TerraformError::CannotDeleteLockFile {
                terraform_provider_lock: terraform_provider_lock.to_string(),
                raw_message: e.to_string(),
            });
        };
        thread::sleep(sleep_time);
        return terraform_init(root_dir);
    }

    if error_string.contains("Plugin reinitialization required") {
        return terraform_init(root_dir);
    }

    Ok(vec![])
}

pub fn force_terraform_ec2_instance_type_switch(
    root_dir: &str,
    error: TerraformError,
    logger: &dyn Logger,
    event_details: &EventDetails,
    dry_run: bool,
) -> Result<Vec<String>, TerraformError> {
    // Error: Failed to change instance type for ec2
    let error_string = error.to_string();

    if error_string.contains("InvalidInstanceType: The following supplied instance types do not exist:")
        && error_string.contains("Error: reading EC2 Instance Type")
    {
        logger.log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Removing invalid instance type".to_string()),
        ));
        terraform_state_rm_entry(root_dir, "aws_instance.ec2_instance")?;
        return terraform_run(TerraformAction::VALIDATE | TerraformAction::APPLY, root_dir, dry_run);
    }

    Err(error)
}

fn terraform_init(root_dir: &str) -> Result<Vec<String>, TerraformError> {
    // issue with provider lock since 0.14 and CI, need to manage terraform lock
    let terraform_provider_lock = format!("{}/.terraform.lock.hcl", &root_dir);
    // no more architectures have been added because of some not availables (mostly on mac os)
    let terraform_providers_lock_args = vec!["providers", "lock", "-platform=linux_amd64"];
    let result = retry::retry(Fixed::from_millis(3000).take(5), || {
        // terraform init
        match terraform_exec(root_dir, terraform_providers_lock_args.clone()) {
            Ok(output) => OperationResult::Ok(output),
            Err(err) => OperationResult::Retry(err),
        }
    });

    match result {
        Ok(_) => {}
        Err(Operation { error, .. }) => return Err(error),
        Err(retry::Error::Internal(e)) => {
            return Err(TerraformError::new(
                terraform_providers_lock_args.iter().map(|e| e.to_string()).collect(),
                "".to_string(),
                e,
            ))
        }
    };

    let terraform_args = vec!["init", "-no-color"];
    let result = retry::retry(Fixed::from_millis(3000).take(5), || {
        // terraform init
        match terraform_exec(root_dir, terraform_args.clone()) {
            Ok(output) => OperationResult::Ok(output),
            Err(err) => {
                let _ = manage_common_issues(root_dir, &terraform_provider_lock, &err);
                // Error while trying to run terraform init, retrying...
                OperationResult::Retry(err)
            }
        }
    });

    match result {
        Ok(output) => Ok(output),
        Err(Operation { error, .. }) => Err(error),
        Err(retry::Error::Internal(e)) => Err(TerraformError::new(
            terraform_args.iter().map(|e| e.to_string()).collect(),
            "".to_string(),
            e,
        )),
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
                let _ = manage_common_issues(root_dir, &terraform_provider_lock, &err);
                // error while trying to Terraform validate on the rendered templates
                OperationResult::Retry(err)
            }
        }
    });

    match result {
        Ok(output) => Ok(output),
        Err(Operation { error, .. }) => Err(error),
        Err(retry::Error::Internal(e)) => Err(TerraformError::new(
            terraform_args.iter().map(|e| e.to_string()).collect(),
            "".to_string(),
            e,
        )),
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
        Err(retry::Error::Internal(e)) => Err(TerraformError::new(
            terraform_args.iter().map(|e| e.to_string()).collect(),
            "".to_string(),
            e,
        )),
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
                let _ = manage_common_issues(root_dir, "", &err);
                // Error while trying to Terraform plan the rendered templates
                OperationResult::Retry(err)
            }
        }
    });

    match result {
        Ok(output) => Ok(output),
        Err(Operation { error, .. }) => Err(error),
        Err(retry::Error::Internal(e)) => Err(TerraformError::new(
            terraform_args.iter().map(|e| e.to_string()).collect(),
            "".to_string(),
            e,
        )),
    }
}

fn terraform_apply(root_dir: &str) -> Result<Vec<String>, TerraformError> {
    let terraform_args = vec!["apply", "-no-color", "-auto-approve", "tf_plan"];
    let result = retry::retry(Fixed::from_millis(3000).take(1), || {
        // ensure we do plan before apply otherwise apply could crash.
        if let Err(e) = terraform_plan(root_dir) {
            return OperationResult::Retry(e);
        };

        // terraform apply
        match terraform_exec(root_dir, terraform_args.clone()) {
            Ok(out) => OperationResult::Ok(out),
            Err(err) => {
                let _ = manage_common_issues(root_dir, "", &err);
                // error while trying to Terraform validate on the rendered templates
                OperationResult::Retry(err)
            }
        }
    });

    match result {
        Ok(output) => Ok(output),
        Err(Operation { error, .. }) => Err(error),
        Err(retry::Error::Internal(e)) => Err(TerraformError::new(
            terraform_args.iter().map(|e| e.to_string()).collect(),
            "".to_string(),
            e,
        )),
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

    let result = retry::retry(Fixed::from_millis(3000).take(1), || {
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
        Err(retry::Error::Internal(e)) => Err(TerraformError::new(terraform_args_string, "".to_string(), e)),
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
    let result = retry::retry(Fixed::from_millis(3000).take(1), || {
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
        Err(retry::Error::Internal(e)) => Err(TerraformError::new(
            terraform_args.iter().map(|e| e.to_string()).collect(),
            "".to_string(),
            e,
        )),
    }
}

// fn terraform_import(root_dir: &str, resource: &str, resource_identifier: &str) -> Result<Vec<String>, TerraformError> {
//     let terraform_args = vec!["import", resource, resource_identifier];
//
//     let result = retry::retry(Fixed::from_millis(3000).take(1), || {
//         // terraform import
//         match terraform_exec(root_dir, terraform_args.clone()) {
//             Ok(output) => OperationResult::Ok(output),
//             Err(err) => {
//                 // Error while trying to run terraform init, retrying...
//                 OperationResult::Retry(err)
//             }
//         }
//     });
//
//     match result {
//         Ok(output) => Ok(output),
//         Err(Operation { error, .. }) => Err(error),
//         Err(retry::Error::Internal(e)) => Err(TerraformError::new(
//             terraform_args.iter().map(|e| e.to_string()).collect(),
//             "".to_string(),
//             e,
//         )),
//     }
// }

// fn terraform_destroy_resource(root_dir: &str, resource: &str) -> Result<Vec<String>, TerraformError> {
//     let terraform_args = vec!["destroy", "-target", resource];
//
//     let result = retry::retry(Fixed::from_millis(3000).take(1), || {
//         // terraform destroy a specific resource
//         match terraform_exec(root_dir, terraform_args.clone()) {
//             Ok(output) => OperationResult::Ok(output),
//             Err(err) => {
//                 // Error while trying to run terraform init, retrying...
//                 OperationResult::Retry(err)
//             }
//         }
//     });
//
//     match result {
//         Ok(output) => Ok(output),
//         Err(Operation { error, .. }) => Err(error),
//         Err(retry::Error::Internal(e)) => Err(TerraformError::new(
//             terraform_args.iter().map(|e| e.to_string()).collect(),
//             "".to_string(),
//             e,
//         )),
//     }
// }

pub fn terraform_remove_resource_from_tf_state(root_dir: &str, resource: &str) -> Result<Vec<String>, TerraformError> {
    let terraform_args = vec!["state", "rm", resource];

    let result = retry::retry(Fixed::from_millis(3000).take(1), || {
        // terraform destroy a specific resource
        match terraform_exec(root_dir, terraform_args.clone()) {
            Ok(output) => OperationResult::Ok(output),
            Err(err) => {
                // Error while trying to run terraform init, retrying...
                OperationResult::Retry(err)
            }
        }
    });

    match result {
        Ok(output) => Ok(output),
        Err(Operation { error, .. }) => Err(error),
        Err(retry::Error::Internal(e)) => Err(TerraformError::new(
            terraform_args.iter().map(|e| e.to_string()).collect(),
            "".to_string(),
            e,
        )),
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

// Temporary ugly stuffs to migrate cloudwatch logs, and being deleted after migration

fn terraform_run_cloudwatch_migration(
    actions: TerraformAction,
    root_dir: &str,
    dry_run: bool,
    cluster_name: &str,
) -> Result<Vec<String>, TerraformError> {
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

    if actions.contains(TerraformAction::MIGRATE_CLOUDWATCH) {
        output.extend(terraform_migrate_cloudwatch(root_dir, cluster_name)?);
    }

    if actions.contains(TerraformAction::APPLY) && !dry_run {
        output.extend(terraform_apply(root_dir)?);
    }

    if actions.contains(TerraformAction::DESTROY) && !dry_run {
        output.extend(terraform_destroy(root_dir)?);
    }

    Ok(output)
}

pub fn terraform_migrate_cloudwatch(root_dir: &str, cluster_name: &str) -> Result<Vec<String>, TerraformError> {
    // get terraform state list output
    let terraform_args = vec!["state", "list"];
    let result = retry::retry(Fixed::from_millis(3000).take(1), || {
        match terraform_exec(root_dir, terraform_args.clone()) {
            Ok(out) => OperationResult::Ok(out),
            Err(err) => {
                // Error while trying to run terraform state list, retrying...
                OperationResult::Retry(err)
            }
        }
    });

    let res = match result {
        Ok(output) => output,
        _ => return Ok(vec![]),
    };

    // check if migration has already been done
    for line in res {
        if line.contains("eks_cloudwatch_log_groups") {
            return Ok(vec![]);
        }
    }

    let cloudwatch_format = format!("/aws/eks/{}/cluster", cluster_name);
    let terraform_migrate_args = vec![
        "import",
        "aws_cloudwatch_log_group.eks_cloudwatch_log_groups",
        &cloudwatch_format,
    ];
    let result = retry::retry(Fixed::from_millis(3000).take(1), || {
        match terraform_exec(root_dir, terraform_migrate_args.clone()) {
            Ok(out) => OperationResult::Ok(out),
            Err(err) => {
                // Error while trying to run terraform state list, retrying...
                OperationResult::Retry(err)
            }
        }
    });

    match result {
        Ok(output) => Ok(output),
        _ => Ok(vec![]),
    }
}

pub fn terraform_init_validate_migrate_cloudwatch_plan_apply(
    root_dir: &str,
    dry_run: bool,
    cluster_name: &str,
) -> Result<Vec<String>, TerraformError> {
    // Terraform init, validate, plan and apply
    terraform_run_cloudwatch_migration(
        TerraformAction::INIT
            | TerraformAction::VALIDATE
            | TerraformAction::MIGRATE_CLOUDWATCH
            | TerraformAction::APPLY,
        root_dir,
        dry_run,
        cluster_name,
    )
}

// End of temporary ugly migration

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
fn terraform_exec_from_command(cmd: &mut impl ExecutableCommand) -> Result<Vec<String>, TerraformError> {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

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

    match result {
        Ok(_) => Ok(stdout),
        Err(_) => Err(TerraformError::new(cmd.get_args(), stdout.join("\n"), stderr.join("\n"))),
    }
}

/// This method should not be exposed to the outside world, it's internal magic.
fn terraform_exec(root_dir: &str, args: Vec<&str>) -> Result<Vec<String>, TerraformError> {
    // override if environment variable is set
    let tf_plugin_cache_dir_value = match env::var_os(TF_PLUGIN_CACHE_DIR) {
        Some(val) => format!("{:?}", val)
            .trim_start_matches('"')
            .trim_end_matches('"')
            .to_string(),
        None => {
            let home_dir = home_dir().expect("Could not find $HOME");
            format!("{}/.terraform.d/plugin-cache", home_dir.to_str().unwrap())
        }
    };

    let envs = &[(TF_PLUGIN_CACHE_DIR, tf_plugin_cache_dir_value.as_str())];
    let mut cmd = QoveryCommand::new("terraform", &args, envs);
    cmd.set_current_dir(root_dir);

    terraform_exec_from_command(&mut cmd)
}

#[cfg(test)]
mod tests {
    use crate::cmd::command::{CommandError, CommandKiller, ExecutableCommand};
    use crate::cmd::terraform::{
        manage_common_issues, terraform_exec_from_command, terraform_init, terraform_init_validate, QuotaExceededError,
        TerraformError,
    };
    use std::fs;
    use std::process::Child;

    use tracing::{span, Level};
    use tracing_test::traced_test;

    // Creating a qovery command mock to fake underlying cli return
    // TODO(benjaminch): This struct is by no mean complete nor polished and has been introduced to investigate an issue. It needs to be polished to be spred elsewhere.
    struct QoveryCommandMock {
        stdout_output: Option<String>,
        stderr_output: Option<String>,
    }

    impl ExecutableCommand for QoveryCommandMock {
        fn get_args(&self) -> Vec<String> {
            vec![]
        }

        fn kill(&self, _cmd_handle: &mut Child) {
            todo!()
        }

        fn exec(&mut self) -> Result<(), CommandError> {
            todo!()
        }

        fn exec_with_output<STDOUT, STDERR>(
            &mut self,
            stdout_output: &mut STDOUT,
            stderr_output: &mut STDERR,
        ) -> Result<(), CommandError>
        where
            STDOUT: FnMut(String),
            STDERR: FnMut(String),
        {
            if let Some(stdout) = &self.stdout_output {
                stdout_output(stdout.to_string());
            }
            if let Some(stderr) = &self.stderr_output {
                stderr_output(stderr.to_string());
            }

            Err(CommandError::TimeoutError("boom!".to_string()))
        }

        fn exec_with_abort<STDOUT, STDERR>(
            &mut self,
            _stdout_output: &mut STDOUT,
            _stderr_output: &mut STDERR,
            _abort_notifier: &CommandKiller,
        ) -> Result<(), CommandError>
        where
            STDOUT: FnMut(String),
            STDERR: FnMut(String),
        {
            todo!()
        }
    }

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
        let result = manage_common_issues("", "/tmp/do_not_exists", &could_not_load_plugin_error);
        assert_eq!(result, terraform_init(""));
    }

    #[test]
    fn test_terraform_truncated_raw_errors() {
        // setup:
        let raw_error_string = r#"Error: creating EC2 Instance: InvalidParameterValue: Invalid value 'wrong-instance-type' for InstanceType.
            status code: 400, request id: 84c20698-c53c-47b3-b840-1a79eacccce6

              on ec2.tf line 21, in resource \"aws_instance\" \"ec2_instance\":\n  21: resource \"aws_instance\" \"ec2_instance\" {

              

                 I am not truncated!

        "#;

        let qovery_cmd_mock = &mut QoveryCommandMock {
            stdout_output: None,
            stderr_output: Some(raw_error_string.to_string()),
        };

        // execute:
        let result = terraform_exec_from_command(qovery_cmd_mock);

        // verify:
        assert_eq!(
            Err(TerraformError::InstanceTypeDoesntExist {
                instance_type: Some("wrong-instance-type".to_string()),
                raw_message: raw_error_string.to_string(),
            }),
            result
        );
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
        fs::create_dir_all(dest_dir).unwrap();

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
        let result = TerraformError::new(vec!["apply".to_string()], "".to_string(), raw_message.to_string());

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
                input_raw_message: "error creating EC2 VPC: VpcLimitExceeded: The maximum number of VPCs has been reached.",
                expected_terraform_error: TerraformError::QuotasExceeded {
                    sub_type: QuotaExceededError::ResourceLimitExceeded {
                        resource_type: "VPC".to_string(),
                        max_resource_count: None,
                    },
                    raw_message: "error creating EC2 VPC: VpcLimitExceeded: The maximum number of VPCs has been reached."
                        .to_string(),
                },
            },
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
                input_raw_message: "Error: Error creating VPC: OptInRequired: You are not subscribed to this service. Please go to http://aws.amazon.com to subscribe.",
                expected_terraform_error: TerraformError::ServiceNotActivatedOptInRequired {
                    raw_message: "Error: Error creating VPC: OptInRequired: You are not subscribed to this service. Please go to http://aws.amazon.com to subscribe.".to_string(),
                    service_type: "VPC".to_string(),
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
            TestCase {
                input_raw_message: "InvalidParameterException: Limit of 30 nodegroups exceeded.",
                expected_terraform_error: TerraformError::QuotasExceeded {
                    sub_type: QuotaExceededError::ResourceLimitExceeded {
                        resource_type: "nodegroups".to_string(),
                        max_resource_count: Some(30),
                    },
                    raw_message: "InvalidParameterException: Limit of 30 nodegroups exceeded.".to_string(),
                },
            },
        ];

        for tc in test_cases {
            // execute:
            let result =
                TerraformError::new(vec!["apply".to_string()], "".to_string(), tc.input_raw_message.to_string());

            // validate:
            assert_eq!(tc.expected_terraform_error, result);
        }
    }

    #[test]
    fn test_terraform_error_resources_issues() {
        // setup:
        struct TestCase<'a> {
            input_raw_std: &'a str,
            input_raw_error: &'a str,
            expected_terraform_error: TerraformError,
        }

        let test_cases = vec![
            TestCase {
                input_raw_std:
                "local_file.qovery_tf_config: Refreshing state... [id=73d5862f0e094563fbe7c49a390a899344dae42d]\\time_static.on_cluster_create: Refreshing state... [id=2022-08-04T15:30:36Z]\\scaleway_k8s_cluster.kubernetes_cluster: Refreshing state... [id=pl-waw/da0cbf08-71dc-4775-8984-5bc84974e8cf]",
                input_raw_error: "Error: scaleway-sdk-go: waiting for cluster failed: timeout after 15m0s",
                expected_terraform_error: TerraformError::WaitingTimeoutResource {
                    resource_type: "scaleway_k8s_cluster.kubernetes_cluster".to_string(),
                    resource_identifier: "pl-waw/da0cbf08-71dc-4775-8984-5bc84974e8cf".to_string(),
                    raw_message:
                    "Error: scaleway-sdk-go: waiting for cluster failed: timeout after 15m0s".to_string(),
                },
            },
            TestCase {
                input_raw_std:
                "scaleway_k8s_cluster.kubernetes_cluster: Creating...",
                input_raw_error: "Error: scaleway-sdk-go: invalid argument(s): name does not respect constraint, cluster name must be unique across the project",
                expected_terraform_error: TerraformError::AlreadyExistingResource {
                    resource_type: "scaleway_k8s_cluster.kubernetes_cluster".to_string(),
                    raw_message:
                    "Error: scaleway-sdk-go: invalid argument(s): name does not respect constraint, cluster name must be unique across the project".to_string(),
                },
            },
        ];

        for tc in test_cases {
            // execute:
            let result = TerraformError::new(
                vec!["apply".to_string()],
                tc.input_raw_std.to_string(),
                tc.input_raw_error.to_string(),
            );

            // validate:
            assert_eq!(tc.expected_terraform_error, result);
        }
    }

    #[test]
    fn test_terraform_error_aws_permissions_issue() {
        // setup:
        let raw_message = "Error: error creating IAM policy qovery-aws-EBS-CSI-Driver-z2242cca3: AccessDenied: User: arn:aws:iam::542561660426:user/thomas is not authorized to perform: iam:CreatePolicy on resource: policy qovery-aws-EBS-CSI-Driver-z2242cca3 because no identity-based policy allows the iam:CreatePolicy action status code: 403, request id: 01ca1501-a0db-438e-a6db-4a2628236cba".to_string();

        // execute:
        let result = TerraformError::new(vec!["apply".to_string()], "".to_string(), raw_message.to_string());

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

    #[test]
    fn test_terraform_error_aws_resource_state_issue() {
        // setup:
        let raw_message = "Error: Error modifying DB instance zabcd-postgresql: InvalidDBInstanceState: You can't modify a stopped DB instance. Start the DB instance, and then modify it".to_string();

        // execute:
        let result = TerraformError::new(vec!["apply".to_string()], "".to_string(), raw_message.to_string());

        // validate:
        assert_eq!(
            TerraformError::WrongExpectedState {
                resource_name: "zabcd-postgresql".to_string(),
                resource_kind: "DB".to_string(),
                raw_message
            },
            result
        );
    }

    #[test]
    fn test_terraform_error_aws_dependency_violation_issue() {
        // setup:
        struct TestCase<'a> {
            input_raw_error: &'a str,
            expected_terraform_error: TerraformError,
        }

        let test_cases = vec![
            TestCase {
                input_raw_error: r#"Error: Error deleting VPC: DependencyViolation: The vpc 'vpc-0330249c67533e3e7' has dependencies and cannot be deleted.
            status code: 400, request id: 2be352ce-4b43-4243-ace7-0b9f2ba35734"#,
                expected_terraform_error: TerraformError::ResourceDependencyViolation {
                    resource_name: "vpc-0330249c67533e3e7".to_string(),
                    resource_kind: "VPC".to_string(),
                    raw_message: r#"Error: Error deleting VPC: DependencyViolation: The vpc 'vpc-0330249c67533e3e7' has dependencies and cannot be deleted.
            status code: 400, request id: 2be352ce-4b43-4243-ace7-0b9f2ba35734"#.to_string(),
                },
            },
            TestCase {
                input_raw_error: r#"Error: deleting EC2 Subnet (subnet-081a519e38fca7bbb): DependencyViolation: The subnet 'subnet-081a519e38fca7bbb' has dependencies and cannot be deleted"#,
                expected_terraform_error: TerraformError::ResourceDependencyViolation {
                    resource_name: "subnet-081a519e38fca7bbb".to_string(),
                    resource_kind: "EC2 Subnet".to_string(),
                    raw_message: r#"Error: deleting EC2 Subnet (subnet-081a519e38fca7bbb): DependencyViolation: The subnet 'subnet-081a519e38fca7bbb' has dependencies and cannot be deleted"#.to_string(),
                },
            }];

        for tc in test_cases {
            // execute:
            let result = TerraformError::new(vec!["apply".to_string()], "".to_string(), tc.input_raw_error.to_string());

            // validate:
            assert_eq!(tc.expected_terraform_error, result);
        }
    }

    #[test]
    fn test_terraform_error_aws_invalid_instance_type() {
        // setup:
        struct TestCase<'a> {
            input_raw_message: &'a str,
            expected_terraform_error: TerraformError,
        }

        let test_cases = vec![
            TestCase {
                input_raw_message:
                "InvalidParameterException: The following supplied instance types do not exist: [t3a.medium]",
                expected_terraform_error: TerraformError::InstanceTypeDoesntExist {
                    instance_type: Some("t3a.medium".to_string()),
                    raw_message: "InvalidParameterException: The following supplied instance types do not exist: [t3a.medium]".to_string(),
                },
            },
            TestCase {
                input_raw_message:
                "Error: creating EC2 Instance: InvalidParameterValue: Invalid value 'wrong-instance-type' for InstanceType",
                expected_terraform_error: TerraformError::InstanceTypeDoesntExist {
                    instance_type: Some("wrong-instance-type".to_string()),
                    raw_message: "Error: creating EC2 Instance: InvalidParameterValue: Invalid value 'wrong-instance-type' for InstanceType".to_string(),
                },
            },
            TestCase {
                input_raw_message:
                "Error: creating EC2 Instance: Unsupported: The requested configuration is currently not supported.",
                expected_terraform_error: TerraformError::InstanceTypeDoesntExist {
                    instance_type: None,
                    raw_message: "Error: creating EC2 Instance: Unsupported: The requested configuration is currently not supported.".to_string(),
                },
            },
        ];

        for tc in test_cases {
            // execute:
            let result =
                TerraformError::new(vec!["apply".to_string()], "".to_string(), tc.input_raw_message.to_string());

            // validate:
            assert_eq!(tc.expected_terraform_error, result);
        }
    }

    #[test]
    fn test_terraform_error_aws_instance_volume_cannot_be_downsized() {
        // setup:
        let raw_terraform_error_str = "Error: updating EC2 Instance (i-0c2ba371783e941e9) volume (vol-0c43352a3d601dc59): InvalidParameterValue: New size cannot be smaller than existing size";

        // execute:
        let result =
            TerraformError::new(vec!["apply".to_string()], "".to_string(), raw_terraform_error_str.to_string());

        // validate:
        assert_eq!(
            TerraformError::InstanceVolumeCannotBeDownSized {
                instance_id: "i-0c2ba371783e941e9".to_string(),
                volume_id: "vol-0c43352a3d601dc59".to_string(),
                raw_message: raw_terraform_error_str.to_string(),
            },
            result
        );
    }

    #[test]
    fn test_terraform_error_multiple_interrupts_received() {
        // setup:
        let raw_message = r#"Two interrupts received. Exiting immediately. Note that data
        loss may have occurred.
        
        Error: rpc error: code = Unavailable desc = transport is closing
        
        
        
        Error: operation canceled"#
            .to_string();

        // execute:
        let result = TerraformError::new(vec!["apply".to_string()], "".to_string(), raw_message.to_string());

        // validate:
        assert_eq!(TerraformError::MultipleInterruptsReceived { raw_message }, result);
    }

    #[test]
    fn test_terraform_error_aws_account_blocked() {
        // setup:
        let raw_message = "Error: creating EC2 Instance: Blocked: This account is currently blocked and not recognized as a valid account. Please contact aws-verification@amazon.com if you have questions."
            .to_string();

        // execute:
        let result = TerraformError::new(vec!["apply".to_string()], "".to_string(), raw_message.to_string());

        // validate:
        assert_eq!(TerraformError::AccountBlockedByProvider { raw_message }, result);
    }

    #[test]
    fn test_terraform_error_state_lock() {
        // setup:
        let raw_terraform_error_str = r#"Error: Error acquiring the state lock
        
Error message: ConditionalCheckFailedException: The conditional request
failed
Lock Info:
  ID:        ecd9f287-8d29-4331-1683-48028be7aaba
  Path:      qovery-terrafom-tfstates/z00007219/qovery-terrafom-tfstates.tfstate
  Operation: OperationTypeApply
  Who:       likornus@likornus
  Version:   1.3.3
  Created:   2022-11-14 13:59:21.540636643 +0000 UTC
  Info:      


Terraform acquires a state lock to protect the state from being written
by multiple users at the same time. Please resolve the issue above and try
again. For most commands, you can disable locking with the "-lock=false"
flag, but this is not recommended."#;

        // execute:
        let result =
            TerraformError::new(vec!["apply".to_string()], "".to_string(), raw_terraform_error_str.to_string());

        // validate:
        assert_eq!(
            TerraformError::StateLocked {
                lock_id: "ecd9f287-8d29-4331-1683-48028be7aaba".to_string(),
                raw_message: raw_terraform_error_str.to_string(),
            },
            result
        );
    }
}

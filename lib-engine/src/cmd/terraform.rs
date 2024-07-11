use bitflags::bitflags;
use dirs::home_dir;
use retry::delay::Fixed;
use retry::OperationResult;

use crate::cmd::command::{ExecutableCommand, QoveryCommand};
use crate::cmd::terraform_validators::{TerraformValidationError, TerraformValidators};
use crate::constants::TF_PLUGIN_CACHE_DIR;
use crate::events::{EngineEvent, EventDetails, EventMessage};
use crate::logger::Logger;
use rand::Rng;
use regex::Regex;
use std::fmt::{Display, Formatter};
use std::{env, fs, thread, time};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct TerraformOutput {
    pub raw_error_output: Vec<String>,
    pub raw_std_output: Vec<String>,
}

impl TerraformOutput {
    pub fn new(raw_std_output: Vec<&str>, raw_error_output: Vec<&str>) -> Self {
        TerraformOutput {
            raw_error_output: raw_error_output.iter().map(|s| s.to_string()).collect(),
            raw_std_output: raw_std_output.iter().map(|s| s.to_string()).collect(),
        }
    }

    pub fn extend(&mut self, output: TerraformOutput) {
        self.raw_error_output.extend(output.raw_error_output);
        self.raw_std_output.extend(output.raw_std_output);
    }
}

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

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum QuotaExceededError {
    ResourceLimitExceeded {
        resource_type: String,
        current_resource_count: Option<u32>,
        max_resource_count: Option<u32>,
    },

    // Cloud provider specifics
    // TODO(benjaminch): variant below this comment might probably not live here on the long run.
    // There is some cloud providers specific errors and it would make more sense to delegate logic
    // identifying those errors (trait implementation) on cloud provider side next to their kubernetes implementation.
    ScwNewAccountNeedsValidation,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum DatabaseError {
    VersionUpgradeNotPossible { from: String, to: String },
    VersionNotSupportedOnTheInstanceType { version: String, db_instance_type: String },
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
        action: Option<String>,
        user: Option<String>,
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
    CannotImportResource {
        resource_type: String,
        resource_identifier: String,
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
        resource_name: Option<String>,
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
    ClusterVersionUnsupportedUpdate {
        cluster_actual_version: String,
        cluster_target_version: String,
        /// raw_message: raw Terraform error message with all details.
        raw_message: String,
    },
    S3BucketAlreadyOwnedByYou {
        bucket_name: String,
        terraform_resource_name: String,
        /// raw_message: raw Terraform error message with all details.
        raw_message: String,
    },
    ManagedDatabaseError {
        database_name: Option<String>,
        database_type: String,
        database_error_sub_type: Box<DatabaseError>,
        /// raw_message: raw Terraform error message with all details.
        raw_message: String,
    },
    ValidatorError {
        validator_name: String,
        validator_description: String,
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
        if let Ok(scw_quotas_exceeded_re) = Regex::new(
            r"Error: scaleway-sdk-go: quota exceeded\(s\): (?P<resource_type>\w+) has reached its quota \((?P<current_resource_count>\d+)/(?P<max_resource_count>\d+)\)",
        ) {
            if let Some(cap) = scw_quotas_exceeded_re.captures(raw_terraform_error_output.as_str()) {
                if let (Some(resource_type), Some(current_resource_count), Some(max_resource_count)) = (
                    cap.name("resource_type").map(|e| e.as_str()),
                    cap.name("current_resource_count").map(|e| e.as_str().parse::<u32>()),
                    cap.name("max_resource_count").map(|e| e.as_str().parse::<u32>()),
                ) {
                    return TerraformError::QuotasExceeded {
                        sub_type: QuotaExceededError::ResourceLimitExceeded {
                            resource_type: resource_type.to_string(),
                            current_resource_count: match current_resource_count {
                                Ok(c) => Some(c),
                                Err(_) => None,
                            },
                            max_resource_count: match max_resource_count {
                                Ok(c) => Some(c),
                                Err(_) => None,
                            },
                        },
                        raw_message: raw_terraform_error_output.to_string(),
                    };
                }
            }
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
                            current_resource_count: None,
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
                            current_resource_count: None,
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
                            current_resource_count: None,
                            max_resource_count: Some(max_resource_count),
                        },
                        raw_message: raw_terraform_error_output.to_string(),
                    };
                }
            }
        }
        if let Ok(aws_quotas_exceeded_re) = Regex::new(
            r" creating EC2 (?P<resource_type>[\w?\s]+): \w+: The maximum number of [\w?\s]+ has been reached",
        ) {
            if let Some(cap) = aws_quotas_exceeded_re.captures(raw_terraform_error_output.as_str()) {
                if let Some(resource_type) = cap.name("resource_type").map(|e| e.as_str()) {
                    return TerraformError::QuotasExceeded {
                        sub_type: QuotaExceededError::ResourceLimitExceeded {
                            resource_type: resource_type.to_string(),
                            current_resource_count: None,
                            max_resource_count: None,
                        },
                        raw_message: raw_terraform_error_output.to_string(),
                    };
                }
            }
        }
        if let Ok(aws_quotas_exceeded_re) =
            Regex::new(r" creating (?P<resource_type>[\w?\s]+): \w+: The maximum number of [\w?\s]+ has been reached")
        {
            if let Some(cap) = aws_quotas_exceeded_re.captures(raw_terraform_error_output.as_str()) {
                if let Some(resource_type) = cap.name("resource_type").map(|e| e.as_str()) {
                    return TerraformError::QuotasExceeded {
                        sub_type: QuotaExceededError::ResourceLimitExceeded {
                            resource_type: resource_type.to_string(),
                            current_resource_count: None,
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
                            current_resource_count: None,
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
            r"Error:? deleting (?P<resource_kind>.+?)(\(.+?\))?: DependencyViolation: .+ '(?P<resource_name>.+?)' has dependencies and cannot be deleted",
        ) {
            if let Some(cap) = aws_state_expected_re.captures(raw_terraform_error_output.as_str()) {
                if let (Some(resource_kind), Some(resource_name)) = (
                    cap.name("resource_kind").map(|e| e.as_str()),
                    cap.name("resource_name").map(|e| e.as_str()),
                ) {
                    return TerraformError::ResourceDependencyViolation {
                        resource_name: resource_name.trim().to_string(),
                        resource_kind: resource_kind.trim().to_string(),
                        raw_message: raw_terraform_error_output.to_string(),
                    };
                }
            }
        }
        if let Ok(aws_state_expected_re) = Regex::new(
            r"Error:? deleting (?P<resource_kind>.+?)(\((?P<resource_name>.+?)\))?:(.+?) DependencyViolation:(.+)? has some mapped public address\(es\)",
        ) {
            if let Some(cap) = aws_state_expected_re.captures(raw_terraform_error_output.as_str()) {
                if let (Some(resource_kind), Some(resource_name)) = (
                    cap.name("resource_kind").map(|e| e.as_str()),
                    cap.name("resource_name").map(|e| e.as_str()),
                ) {
                    return TerraformError::ResourceDependencyViolation {
                        resource_name: resource_name.trim().to_string(),
                        resource_kind: resource_kind.trim().to_string(),
                        raw_message: raw_terraform_error_output.to_string(),
                    };
                }
            }
        }

        // Invalid credentials issues
        // SCW
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
                        user: Some(user.to_string()),
                        action: Some(action.to_string()),
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
        // ResourceInUseException: Cluster already exists with name: xxx
        if let Ok(cluster_name_regex) =
            Regex::new(r"Error: creating (?P<resource_type>.*) \((?P<resource_name>[-\w]+)\): ResourceInUseException")
        {
            if let Some(cap) = cluster_name_regex.captures(raw_terraform_error_output.as_str()) {
                if let (Some(resource_type), Some(resource_name)) = (
                    cap.name("resource_type").map(|e| e.as_str()),
                    cap.name("resource_name").map(|e| e.as_str()),
                ) {
                    return TerraformError::AlreadyExistingResource {
                        resource_type: resource_type.to_string(),
                        resource_name: Some(resource_name.to_string()),
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
                r"(?P<resource_type>\bscaleway_(?:.*)): Refreshing state... \[id=(?P<resource_identifier>.*)\]",
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

        if raw_terraform_error_output.contains("scaleway-sdk-go: insufficient permissions:") {
            if let Ok(scw_resource) = Regex::new(r"with (?P<resource>\b(?:\w*.\w*))") {
                if let Some(cap) = scw_resource.captures(raw_terraform_error_output.as_str()) {
                    if let Some(resource) = cap.name("resource").map(|e| e.as_str()) {
                        return TerraformError::NotEnoughPermissions {
                            resource_type_and_name: resource.to_string(),
                            raw_message: raw_terraform_error_output,
                            user: None,
                            action: None,
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
                            resource_name: None,
                            raw_message: raw_terraform_error_output,
                        };
                    }
                }
            }
        }

        // Resources creation errors
        // AWS
        // BucketAlreadyOwnedByYou: S3 bucket cannot be created because it already exists. It might happen if Terraform lost connection before writing to the state.
        if let Ok(bucket_name_re) = Regex::new(
            r#"Error: creating Amazon S3 \(Simple Storage\) Bucket \((?P<bucket_name>.+?)\): BucketAlreadyOwnedByYou: Your previous request to create the named bucket succeeded and you already own it(?s:.)*in resource "aws_s3_bucket" "(?P<terraform_resource_name>.+?)":"#,
        ) {
            if let Some(cap) = bucket_name_re.captures(raw_terraform_error_output.as_str()) {
                if let (Some(bucket_name), Some(terraform_resource_name)) = (
                    cap.name("bucket_name").map(|e| e.as_str()),
                    cap.name("terraform_resource_name").map(|e| e.as_str()),
                ) {
                    return TerraformError::S3BucketAlreadyOwnedByYou {
                        bucket_name: bucket_name.to_string(),
                        terraform_resource_name: terraform_resource_name.to_string(),
                        raw_message: raw_terraform_error_output.to_string(),
                    };
                }
            }
        }

        // Managed database errors
        // AWS
        // InvalidParameterCombination: Cannot upgrade docdb from 4.0.0 to 5.0.0
        if let Ok(managed_db_upgrade_error_re) = Regex::new(
            r"Error: Failed to modify [\w\W]+ \((?P<database_name>.+?)\): InvalidParameterCombination: Cannot upgrade (?P<database_type>[\w\W]+) from (?P<version_from>[\d\.]+) to (?P<version_to>[\d\.]+)",
        ) {
            if let Some(cap) = managed_db_upgrade_error_re.captures(raw_terraform_error_output.as_str()) {
                if let (Some(database_name), Some(database_type), Some(version_from), Some(version_to)) = (
                    cap.name("database_name").map(|e| e.as_str()),
                    cap.name("database_type").map(|e| e.as_str()),
                    cap.name("version_from").map(|e| e.as_str()),
                    cap.name("version_to").map(|e| e.as_str()),
                ) {
                    return TerraformError::ManagedDatabaseError {
                        database_name: Some(database_name.to_string()),
                        database_type: database_type.to_string(),
                        database_error_sub_type: Box::new(DatabaseError::VersionUpgradeNotPossible {
                            from: version_from.to_string(),
                            to: version_to.to_string(),
                        }),
                        raw_message: raw_terraform_error_output.to_string(),
                    };
                }
            }
        }
        // InvalidParameterCombination: The combination of the cluster class 'cache.t4g.micro', cache engine 'redis' and cache engine version '5.0.6' is not supported
        if let Ok(managed_db_version_instance_type_incompatible_error_re) = Regex::new(
            r"InvalidParameterCombination: The combination of [\w\s]+ '(?P<database_instance_type>.+?)', [\w\s]+ '(?P<database_type>.+?)' and [\w\s]+ version '(?P<database_engine_version>.+?)' is not supported",
        ) {
            if let Some(cap) =
                managed_db_version_instance_type_incompatible_error_re.captures(raw_terraform_error_output.as_str())
            {
                if let (Some(database_instance_type), Some(database_type), Some(database_engine_version)) = (
                    cap.name("database_instance_type").map(|e| e.as_str()),
                    cap.name("database_type").map(|e| e.as_str()),
                    cap.name("database_engine_version").map(|e| e.as_str()),
                ) {
                    return TerraformError::ManagedDatabaseError {
                        database_name: None,
                        database_type: database_type.to_string(),
                        database_error_sub_type: Box::new(DatabaseError::VersionNotSupportedOnTheInstanceType {
                            version: database_engine_version.to_string(),
                            db_instance_type: database_instance_type.to_string(),
                        }),
                        raw_message: raw_terraform_error_output.to_string(),
                    };
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

        // Cluster version update is not supported (most likely Qovery is trying to deploy an earlier version)
        if let Ok(unsupported_k8s_version_update_re) = Regex::new(
            r"Unsupported Kubernetes minor version update from (?P<cluster_actual_version>[0-9.]+) to (?P<cluster_target_version>[0-9.]+)",
        ) {
            if let Some(cap) = unsupported_k8s_version_update_re.captures(raw_terraform_error_output.as_str()) {
                if let (Some(cluster_actual_version), Some(cluster_target_version)) = (
                    cap.name("cluster_actual_version").map(|e| e.as_str()),
                    cap.name("cluster_target_version").map(|e| e.as_str()),
                ) {
                    return TerraformError::ClusterVersionUnsupportedUpdate {
                        cluster_actual_version: cluster_actual_version.to_string(),
                        cluster_target_version: cluster_target_version.to_string(),
                        raw_message: raw_terraform_error_output.to_string(),
                    };
                }
            }
        }

        // This kind of error should be triggered as little as possible, ideally, there is no unknown errors
        // (un-caught) so we can act / report properly to the user.
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
            TerraformError::AccountBlockedByProvider { .. } => "Your account has been blocked by cloud provider.".to_string(),
            TerraformError::InvalidCredentials { .. } => "Invalid credentials.".to_string(),
            TerraformError::NotEnoughPermissions {
                resource_type_and_name,
                user,
                action,
                ..
            } => match (user, action) {
                (Some(user_value), Some(action_value)) => format!(
                    "Error, user `{user_value}` cannot perform `{action_value}` on `{resource_type_and_name}`."
                ),
                _ => format!(
                    "Error, cannot perform action due to permission on `{resource_type_and_name}`."
                ),
            }
            TerraformError::CannotDeleteLockFile {
                terraform_provider_lock,
                ..
            } => format!("Wasn't able to delete terraform lock file `{terraform_provider_lock}`.",),
            TerraformError::ConfigFileNotFound { path, .. } => {
                format!("Error while trying to get Terraform configuration file `{path}`.",)
            }
            TerraformError::ConfigFileInvalidContent { path, .. } => {
                format!(
                    "Error while trying to read Terraform configuration file, content is invalid `{path}`.",
                )
            }
            TerraformError::CannotRemoveEntryOutOfStateList {
                entry_to_be_removed, ..
            } => {
                format!("Error while trying to remove entry `{entry_to_be_removed}` from state list.",)
            }
            TerraformError::ContextUnsupportedParameterValue {
                service_type,
                parameter_name,
                parameter_value,
                ..
            } => {
                format!(
                    "Error {service_type} value `{parameter_value}` not supported for parameter `{parameter_name}`.",
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
                            current_resource_count,
                            max_resource_count,
                        } => format!(
                            "`{}` has reached its quotas{}.",
                            resource_type,
                            match (current_resource_count, max_resource_count) {
                                (Some(current), Some(max)) => format!(": ({}/{})", current, max),
                                (Some(current), None) => format!(", current count = {}", current),
                                (None, Some(max)) => format!(" of {}", max),
                                (None, None) => "".to_string(),
                            },
                        ),
                    },
                )
            }
            TerraformError::ServiceNotActivatedOptInRequired { service_type, .. } => {
                format!("Error, service `{service_type}` requiring an opt-in is not activated.",)
            }
            TerraformError::AlreadyExistingResource { resource_type, resource_name, .. } => {
                match resource_name {
                    Some(name) => format!("Error, resource type `{resource_type}` with name `{name}` already exists."),
                    None => format!("Error, resource type `{resource_type}` already exists."),
                }
            }
            TerraformError::ResourceDependencyViolation { resource_name, resource_kind, .. } => {
                format!("Error, resource {resource_kind} `{resource_name}` has dependency violation.")
            }
            TerraformError::WaitingTimeoutResource {
                resource_type,
                resource_identifier,
                ..
            } => {
                format!("Error, waiting for resource {resource_type}:{resource_identifier} timeout.")
            }
            TerraformError::WrongExpectedState {
                resource_name: resource_type,
                resource_kind,
                raw_message,
            } => format!("Error, resource {resource_type}:{resource_kind} was expected to be in another state. It happens when changes have been done Cloud provider side without Qovery. You need to fix it manually: {raw_message}"),
            TerraformError::InstanceTypeDoesntExist { instance_type, ..} => format!("Error, requested instance type{} doesn't exist in cluster region.", match instance_type {
                Some(instance_type) => format!(" `{instance_type}`"),
                None => "".to_string(),
            }),
            TerraformError::InstanceVolumeCannotBeDownSized { instance_id, volume_id, .. } => {
                format!("Error, instance (`{instance_id}`) volume (`{volume_id}`) cannot be smaller than existing size.")
            },
            TerraformError::InvalidCIDRBlock {cidr,..} => {
                format!("Error, the CIDR block `{cidr}` can't be used.")
            },
            TerraformError::S3BucketAlreadyOwnedByYou {bucket_name, .. } => {
                format!("Error, the S3 bucket `{bucket_name}` cannot be created, it already exists.")
            }
            TerraformError::StateLocked { lock_id, .. } => {
                format!("Error, terraform state is locked (lock_id: {lock_id})")
            },
            TerraformError::ClusterVersionUnsupportedUpdate { cluster_actual_version, cluster_target_version, .. } => {
                format!("Error, cluster version cannot be updated from `{cluster_actual_version}` to `{cluster_target_version}`")
            },
            TerraformError::CannotImportResource { resource_type, resource_identifier, .. } => {
                format!("Error, cannot import Terraform resource `{resource_identifier}` type `{resource_type}`")
            },
            TerraformError::ManagedDatabaseError { database_name, database_type, database_error_sub_type, .. } => {
                match **database_error_sub_type {
                    DatabaseError::VersionUpgradeNotPossible { ref from, ref to } => {
                        match database_name {
                            Some(name) => format!("Error, cannot perform `{database_type}` database version upgrade from `{from}` to `{to}` on `{name}`"),
                            None => format!("Error, cannot perform `{database_type}` database version upgrade from `{from}` to `{to}`"),
                        }
                    },
                    DatabaseError::VersionNotSupportedOnTheInstanceType { ref version,ref db_instance_type } => format!("Error, `{database_type}` version `{version}` is not compatible with instance type `{db_instance_type}`"),
                }
            },
            TerraformError::ValidatorError { validator_name, validator_description: validation_description,  raw_message, .. } => {
                format!("Error, validator `{validator_name}` ({validation_description}) has raised an error: {raw_message}")
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
            TerraformError::S3BucketAlreadyOwnedByYou { raw_message, .. } => {
                format!("{}\n{}", self.to_safe_message(), raw_message)
            }
            TerraformError::StateLocked { raw_message, .. } => {
                format!("{}\n{}", self.to_safe_message(), raw_message)
            }
            TerraformError::ClusterVersionUnsupportedUpdate { raw_message, .. } => {
                format!("{}\n{}", self.to_safe_message(), raw_message)
            }
            TerraformError::CannotImportResource { raw_message, .. } => {
                format!("{}\n{}", self.to_safe_message(), raw_message)
            }
            TerraformError::ManagedDatabaseError { raw_message, .. } => {
                format!("{}\n{}", self.to_safe_message(), raw_message)
            }
            TerraformError::ValidatorError { raw_message, .. } => {
                format!("{}\n{}", self.to_safe_message(), raw_message)
            }
        };

        f.write_str(&message)
    }
}

impl From<TerraformValidationError> for TerraformError {
    fn from(error: TerraformValidationError) -> Self {
        match error {
            TerraformValidationError::HasForbiddenDestructiveChanges {
                validator_name,
                validator_description,
                resource,
                raw_output,
            } => TerraformError::ValidatorError {
                validator_name,
                validator_description,
                raw_message: format!("Validation error on resource `{}`: {}", resource, raw_output),
            },
        }
    }
}

fn manage_common_issues(
    root_dir: &str,
    terraform_provider_lock: &str,
    err: &TerraformError,
    validators: &TerraformValidators,
) -> Result<TerraformOutput, TerraformError> {
    terraform_plugins_failed_load(root_dir, err, terraform_provider_lock, validators)?;

    Ok(TerraformOutput::default())
}

fn terraform_plugins_failed_load(
    root_dir: &str,
    error: &TerraformError,
    terraform_provider_lock: &str,
    validators: &TerraformValidators,
) -> Result<TerraformOutput, TerraformError> {
    let output = TerraformOutput::default();

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
        return terraform_init(root_dir, &[], validators);
    }

    if error_string.contains("Plugin reinitialization required") {
        return terraform_init(root_dir, &[], validators);
    }

    Ok(output)
}

pub fn force_terraform_ec2_instance_type_switch(
    root_dir: &str,
    error: TerraformError,
    logger: &dyn Logger,
    event_details: &EventDetails,
    dry_run: bool,
    envs: &[(&str, &str)],
    validators: &TerraformValidators,
) -> Result<TerraformOutput, TerraformError> {
    // Error: Failed to change instance type for ec2
    let error_string = error.to_string();

    if error_string.contains("InvalidInstanceType: The following supplied instance types do not exist:")
        && error_string.contains("Error: reading EC2 Instance Type")
    {
        logger.log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Removing invalid instance type".to_string()),
        ));
        terraform_state_rm_entry(root_dir, "aws_instance.ec2_instance", validators)?;
        return terraform_run(
            TerraformAction::VALIDATE | TerraformAction::APPLY,
            root_dir,
            dry_run,
            envs,
            validators,
        );
    }

    Err(error)
}

fn terraform_init(
    root_dir: &str,
    envs: &[(&str, &str)],
    validators: &TerraformValidators,
) -> Result<TerraformOutput, TerraformError> {
    // issue with provider lock since 0.14 and CI, need to manage terraform lock
    let terraform_provider_lock = format!("{}/.terraform.lock.hcl", &root_dir);

    // no more architectures have been added because of some not availables (mostly on mac os)
    let mut terraform_providers_lock_args = vec!["providers", "lock"];
    #[cfg(target_os = "macos")]
    terraform_providers_lock_args.push("-platform=darwin_arm64");
    #[cfg(target_os = "linux")]
    terraform_providers_lock_args.push("-platform=linux_amd64");
    #[cfg(target_os = "linux")]
    terraform_providers_lock_args.push("-platform=linux_arm64");

    let result = retry::retry(Fixed::from_millis(3000).take(5), || {
        // terraform init
        match terraform_exec(root_dir, terraform_providers_lock_args.clone(), envs, validators) {
            Ok(output) => OperationResult::Ok(output),
            Err(err) => OperationResult::Retry(err),
        }
    });

    match result {
        Ok(_) => {}
        Err(retry::Error { error, .. }) => return Err(error),
    };

    let terraform_args = vec!["init", "-no-color"];
    let result = retry::retry(Fixed::from_millis(3000).take(5), || {
        // terraform init
        match terraform_exec(root_dir, terraform_args.clone(), envs, validators) {
            Ok(output) => OperationResult::Ok(output),
            Err(err) => {
                let _ = manage_common_issues(root_dir, &terraform_provider_lock, &err, validators);
                // Error while trying to run terraform init, retrying...
                OperationResult::Retry(err)
            }
        }
    });

    match result {
        Ok(output) => Ok(output),
        Err(retry::Error { error, .. }) => Err(error),
    }
}

fn terraform_validate(
    root_dir: &str,
    envs: &[(&str, &str)],
    validators: &TerraformValidators,
) -> Result<TerraformOutput, TerraformError> {
    let terraform_args = vec!["validate", "-no-color"];
    let terraform_provider_lock = format!("{}/.terraform.lock.hcl", &root_dir);

    // Retry is not needed, fixing it to 1 only for the time being
    let result = retry::retry(Fixed::from_millis(3000).take(1), || {
        // validate config
        match terraform_exec(root_dir, terraform_args.clone(), envs, validators) {
            Ok(output) => OperationResult::Ok(output),
            Err(err) => {
                let _ = manage_common_issues(root_dir, &terraform_provider_lock, &err, validators);
                // error while trying to Terraform validate on the rendered templates
                OperationResult::Retry(err)
            }
        }
    });

    match result {
        Ok(output) => Ok(output),
        Err(retry::Error { error, .. }) => Err(error),
    }
}

pub fn terraform_state_list(
    root_dir: &str,
    envs: &[(&str, &str)],
    validators: &TerraformValidators,
) -> Result<TerraformOutput, TerraformError> {
    // get terraform state list output
    let terraform_args = vec!["state", "list"];
    let result = retry::retry(Fixed::from_millis(3000).take(5), || {
        match terraform_exec(root_dir, terraform_args.clone(), envs, validators) {
            Ok(out) => OperationResult::Ok(out),
            Err(err) => {
                // Error while trying to run terraform state list, retrying...
                OperationResult::Retry(err)
            }
        }
    });

    match result {
        Ok(output) => Ok(output),
        Err(retry::Error { error, .. }) => Err(error),
    }
}

pub fn terraform_plan(
    root_dir: &str,
    envs: &[(&str, &str)],
    validators: &TerraformValidators,
) -> Result<TerraformOutput, TerraformError> {
    // plan
    let terraform_args = vec!["plan", "-no-color", "-out", "tf_plan"];
    // Retry is not needed, fixing it to 1 only for the time being
    let result = retry::retry(Fixed::from_millis(3000).take(1), || {
        match terraform_exec(root_dir, terraform_args.clone(), envs, validators) {
            Ok(out) => OperationResult::Ok(out),
            Err(err) => {
                let _ = manage_common_issues(root_dir, "", &err, validators);
                // Error while trying to Terraform plan the rendered templates
                OperationResult::Retry(err)
            }
        }
    });

    match result {
        Ok(output) => Ok(output),
        Err(retry::Error { error, .. }) => Err(error),
    }
}

fn terraform_apply(
    root_dir: &str,
    envs: &[(&str, &str)],
    validators: &TerraformValidators,
) -> Result<TerraformOutput, TerraformError> {
    let terraform_args = vec!["apply", "-lock=false", "-no-color", "-auto-approve", "tf_plan"];
    let result = retry::retry(Fixed::from_millis(3000).take(1), || {
        // ensure we do plan before apply otherwise apply could crash.
        if let Err(e) = terraform_plan(root_dir, envs, validators) {
            return OperationResult::Retry(e);
        };

        // terraform apply
        match terraform_exec(root_dir, terraform_args.clone(), envs, validators) {
            Ok(out) => OperationResult::Ok(out),
            Err(err) => {
                let _ = manage_common_issues(root_dir, "", &err, validators);
                // error while trying to Terraform validate on the rendered templates
                OperationResult::Retry(err)
            }
        }
    });

    match result {
        Ok(output) => Ok(output),
        Err(retry::Error { error, .. }) => Err(error),
    }
}

pub fn terraform_apply_with_tf_workers_resources(
    root_dir: &str,
    tf_workers_resources: Vec<String>,
    envs: &[(&str, &str)],
    validators: &TerraformValidators,
) -> Result<TerraformOutput, TerraformError> {
    let mut terraform_args_string = vec![
        "apply".to_string(),
        "-lock=false".to_string(),
        "-auto-approve".to_string(),
    ];
    for x in tf_workers_resources {
        terraform_args_string.push(format!("-target={x}"));
    }

    let result = retry::retry(Fixed::from_millis(3000).take(1), || {
        // terraform plan first
        if let Err(err) = terraform_plan(root_dir, envs, validators) {
            return OperationResult::Retry(err);
        }

        // terraform apply
        match terraform_exec(
            root_dir,
            terraform_args_string.iter().map(|e| e.as_str()).collect(),
            envs,
            validators,
        ) {
            Ok(out) => OperationResult::Ok(out),
            Err(err) => {
                // Error while trying to run terraform apply on rendered templates, retrying...
                OperationResult::Retry(err)
            }
        }
    });

    match result {
        Ok(output) => Ok(output),
        Err(retry::Error { error, .. }) => Err(error),
    }
}

pub fn terraform_state_rm_entry(
    root_dir: &str,
    entry: &str,
    validators: &TerraformValidators,
) -> Result<TerraformOutput, TerraformError> {
    match terraform_exec(root_dir, vec!["state", "rm", entry], &[], validators) {
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

pub fn terraform_destroy(
    root_dir: &str,
    envs: &[(&str, &str)],
    validators: &TerraformValidators,
) -> Result<TerraformOutput, TerraformError> {
    // terraform destroy
    let terraform_args = vec!["destroy", "-lock=false", "-no-color", "-auto-approve"];
    let result = retry::retry(Fixed::from_millis(3000).take(1), || {
        // terraform plan first
        if let Err(err) = terraform_plan(root_dir, envs, validators) {
            return OperationResult::Retry(err);
        }

        // terraform destroy
        match terraform_exec(root_dir, terraform_args.clone(), envs, validators) {
            Ok(out) => OperationResult::Ok(out),
            Err(err) => {
                // Error while trying to run terraform destroy on rendered templates, retrying...
                OperationResult::Retry(err)
            }
        }
    });

    match result {
        Ok(output) => Ok(output),
        Err(retry::Error { error, .. }) => Err(error),
    }
}

pub fn terraform_import(
    root_dir: &str,
    resource: &str,
    resource_identifier: &str,
    envs: &[(&str, &str)],
    validators: &TerraformValidators,
) -> Result<TerraformOutput, TerraformError> {
    let terraform_args = vec!["import", resource, resource_identifier];

    // terraform import
    match terraform_exec(root_dir, terraform_args.clone(), envs, validators) {
        Ok(output) => Ok(output),
        Err(err) => Err(TerraformError::CannotImportResource {
            resource_type: resource.to_string(),
            resource_identifier: resource_identifier.to_string(),
            raw_message: err.to_string(),
        }),
    }
}

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

pub fn terraform_remove_resource_from_tf_state(
    root_dir: &str,
    resource: &str,
    validators: &TerraformValidators,
) -> Result<TerraformOutput, TerraformError> {
    let terraform_args = vec!["state", "rm", resource];

    let result = retry::retry(Fixed::from_millis(3000).take(1), || {
        // terraform destroy a specific resource
        match terraform_exec(root_dir, terraform_args.clone(), &[], validators) {
            Ok(output) => OperationResult::Ok(output),
            Err(err) => {
                // Error while trying to run terraform init, retrying...
                OperationResult::Retry(err)
            }
        }
    });

    match result {
        Ok(output) => Ok(output),
        Err(retry::Error { error, .. }) => Err(error),
    }
}

fn terraform_run(
    actions: TerraformAction,
    root_dir: &str,
    dry_run: bool,
    envs: &[(&str, &str)],
    validators: &TerraformValidators,
) -> Result<TerraformOutput, TerraformError> {
    let mut output = TerraformOutput::default();

    if actions.contains(TerraformAction::INIT) {
        output.extend(terraform_init(root_dir, envs, validators)?);
    }

    if actions.contains(TerraformAction::VALIDATE) {
        output.extend(terraform_validate(root_dir, envs, validators)?);
    }

    if actions.contains(TerraformAction::STATE_LIST) {
        output.extend(terraform_state_list(root_dir, envs, validators)?);
    }

    if actions.contains(TerraformAction::APPLY) && !dry_run {
        output.extend(terraform_apply(root_dir, envs, validators)?);
    }

    if actions.contains(TerraformAction::DESTROY) && !dry_run {
        output.extend(terraform_destroy(root_dir, envs, validators)?);
    }

    Ok(output)
}

pub fn terraform_init_validate_plan_apply(
    root_dir: &str,
    dry_run: bool,
    envs: &[(&str, &str)],
    validators: &TerraformValidators,
) -> Result<TerraformOutput, TerraformError> {
    // Terraform init, validate, plan and apply
    terraform_run(
        TerraformAction::INIT | TerraformAction::VALIDATE | TerraformAction::APPLY,
        root_dir,
        dry_run,
        envs,
        validators,
    )
}

pub fn terraform_init_validate(
    root_dir: &str,
    envs: &[(&str, &str)],
    validators: &TerraformValidators,
) -> Result<TerraformOutput, TerraformError> {
    // Terraform init & validate
    terraform_run(
        TerraformAction::INIT | TerraformAction::VALIDATE,
        root_dir,
        false,
        envs,
        validators,
    )
}

pub fn terraform_init_validate_destroy(
    root_dir: &str,
    run_apply_before_destroy: bool,
    envs: &[(&str, &str)],
    validators: &TerraformValidators,
) -> Result<TerraformOutput, TerraformError> {
    let mut terraform_actions_to_be_performed = TerraformAction::INIT | TerraformAction::VALIDATE;

    // better to apply before destroy to ensure terraform destroy will delete on all resources
    if run_apply_before_destroy {
        terraform_actions_to_be_performed |= TerraformAction::APPLY;
    }

    terraform_run(
        terraform_actions_to_be_performed | TerraformAction::DESTROY,
        root_dir,
        false,
        envs,
        validators,
    )
}

pub fn terraform_init_validate_state_list(
    root_dir: &str,
    envs: &[(&str, &str)],
    validators: &TerraformValidators,
) -> Result<TerraformOutput, TerraformError> {
    // Terraform init, validate and statelist
    terraform_run(
        TerraformAction::INIT | TerraformAction::VALIDATE | TerraformAction::STATE_LIST,
        root_dir,
        false,
        envs,
        validators,
    )
}

/// This method should not be exposed to the outside world, it's internal magic.
///
/// validators are injected here not to pollute the whole API, but can be exposed to the outside world if needed
fn terraform_exec_from_command(
    cmd: &mut impl ExecutableCommand,
    validators: &TerraformValidators,
) -> Result<TerraformOutput, TerraformError> {
    let mut terraform_output = TerraformOutput::default();

    let result = cmd.exec_with_output(
        &mut |line| {
            info!("{}", line);
            terraform_output.raw_std_output.push(line);
        },
        &mut |line| {
            error!("{}", line);
            terraform_output.raw_error_output.push(line);
        },
    );

    validators.validate(&terraform_output).map_err(TerraformError::from)?;

    match result {
        Ok(_) => Ok(terraform_output),
        Err(_) => Err(TerraformError::new(
            cmd.get_args(),
            terraform_output.raw_std_output.join("\n"),
            terraform_output.raw_error_output.join("\n"),
        )),
    }
}

/// This method should not be exposed to the outside world, it's internal magic.
fn terraform_exec(
    root_dir: &str,
    args: Vec<&str>,
    env: &[(&str, &str)],
    validators: &TerraformValidators,
) -> Result<TerraformOutput, TerraformError> {
    // override if environment variable is set
    let tf_plugin_cache_dir_value = match env::var_os(TF_PLUGIN_CACHE_DIR) {
        Some(val) => format!("{val:?}")
            .trim_start_matches('"')
            .trim_end_matches('"')
            .to_string(),
        None => {
            let home_dir = home_dir().expect("Could not find $HOME");
            format!("{}/.terraform.d/plugin-cache", home_dir.to_str().unwrap())
        }
    };

    let mut envs = vec![(TF_PLUGIN_CACHE_DIR, tf_plugin_cache_dir_value.as_str())];
    envs.extend(env);
    let mut cmd = QoveryCommand::new("terraform", &args, &envs);
    cmd.set_current_dir(root_dir);

    terraform_exec_from_command(&mut cmd, validators)
}

#[cfg(test)]
mod tests {
    use crate::cmd::command::{CommandError, CommandKiller, ExecutableCommand};
    use crate::cmd::terraform::{
        manage_common_issues, terraform_exec_from_command, terraform_init, terraform_init_validate, DatabaseError,
        QuotaExceededError, TerraformError, TerraformOutput,
    };
    use std::fs;
    use std::process::Child;

    use crate::cmd::terraform_validators::{TerraformValidationError, TerraformValidator, TerraformValidators};
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

        let terraform_args = ["apply"];

        let could_not_load_plugin_error = TerraformError::Unknown {
            terraform_args: terraform_args.iter().map(|e| e.to_string()).collect(),
            raw_message: could_not_load_plugin.to_string(),
        };
        let result = manage_common_issues(
            "",
            "/tmp/do_not_exists",
            &could_not_load_plugin_error,
            &TerraformValidators::None,
        );
        assert_eq!(result, terraform_init("", &[], &TerraformValidators::Default));
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
        let result = terraform_exec_from_command(qovery_cmd_mock, &TerraformValidators::Default);

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

provider "registry.terraform.io/hashicorp/time" {
  version     = "0.9.0"
  constraints = "~> 0.9"
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
      source = "hashicorp/time"
      version = "~> 0.9"
    }
  }
  required_version = ">= 0.14"
}
        "#;

        let dest_dir = "/tmp/test";
        fs::create_dir_all(dest_dir).unwrap();

        let _ = fs::write(format!("{}/.terraform.lock.hcl", &dest_dir), terraform_lock_file);
        let _ = fs::write(format!("{}/providers.tf", &dest_dir), provider_file);

        let res = terraform_init_validate(dest_dir, &[], &TerraformValidators::Default);

        assert!(res.is_ok());
    }

    #[test]
    fn test_terraform_error_scw_new_account_quotas_issue() {
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
    fn test_terraform_error_scw_quotas_issue() {
        // setup:
        let raw_message =
            r#"Error: scaleway-sdk-go: quota exceeded(s): CpServersType_PRO2_XXS has reached its quota (0/1)
         with scaleway_k8s_pool.kubernetes_cluster_workers_1,
         on ks-workers-nodes.tf line 2, in resource "scaleway_k8s_pool" "kubernetes_cluster_workers_1":
          2: resource "scaleway_k8s_pool" "kubernetes_cluster_workers_1" {"#
                .to_string();

        // execute:
        let result = TerraformError::new(vec!["apply".to_string()], "".to_string(), raw_message.to_string());

        // validate:
        assert_eq!(
            TerraformError::QuotasExceeded {
                sub_type: QuotaExceededError::ResourceLimitExceeded {
                    current_resource_count: Some(0u32),
                    max_resource_count: Some(1u32),
                    resource_type: "CpServersType_PRO2_XXS".to_string(),
                },
                raw_message
            },
            result
        );
    }

    #[test]
    fn test_terraform_error_scw_permissions_issue() {
        // setup:
        let raw_message = "Error: scaleway-sdk-go: insufficient permissions: \n\n  with scaleway_k8s_cluster.kubernetes_cluster,\n  on ks-master-cluster.tf line 1, in resource \"scaleway_k8s_cluster\" \"kubernetes_cluster\":\n   1: resource \"scaleway_k8s_cluster\" \"kubernetes_cluster".to_string();

        // execute:
        let result = TerraformError::new(vec!["apply".to_string()], "".to_string(), raw_message.to_string());

        // validate:
        assert_eq!(
            TerraformError::NotEnoughPermissions {
                resource_type_and_name: "scaleway_k8s_cluster.kubernetes_cluster".to_string(),
                user: None,
                action: None,
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
                input_raw_message: "Error: creating EC2 VPC: VpcLimitExceeded: The maximum number of VPCs has been reached",
                expected_terraform_error: TerraformError::QuotasExceeded {
                    sub_type: QuotaExceededError::ResourceLimitExceeded {
                        resource_type: "VPC".to_string(),
                        current_resource_count: None,
                        max_resource_count: None,
                    },
                    raw_message: "Error: creating EC2 VPC: VpcLimitExceeded: The maximum number of VPCs has been reached"
                        .to_string(),
                },
            },
            TestCase {
                input_raw_message: "error creating EC2 VPC: VpcLimitExceeded: The maximum number of VPCs has been reached.",
                expected_terraform_error: TerraformError::QuotasExceeded {
                    sub_type: QuotaExceededError::ResourceLimitExceeded {
                        resource_type: "VPC".to_string(),
                        current_resource_count: None,
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
                        current_resource_count: None,
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
                        current_resource_count: None,
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
                        current_resource_count: None,
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
                        current_resource_count: None,
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
                        current_resource_count: None,
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
                        current_resource_count: None,
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
    fn test_terraform_error_scw_resources_issues() {
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
                    resource_name: None,
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
    fn test_terraform_error_aws_managed_db_issues() {
        // setup:
        struct TestCase<'a> {
            input_raw_error: &'a str,
            expected_terraform_error: TerraformError,
        }

        let test_cases = vec![
            TestCase {
                input_raw_error: r#"Error: Failed to modify DocDB Cluster (ze73ae545-mongodb): InvalidParameterCombination: Cannot upgrade docdb from 4.0.0 to 5.0.0
        status code: 400, request id: f6f2f684-4994-45a7-a29a-b75bbb3ebb1b

  with aws_docdb_cluster.documentdb_cluster,
  on main.tf line 45, in resource "aws_docdb_cluster" "documentdb_cluster":
  45: resource "aws_docdb_cluster" "documentdb_cluster" {"#,
                expected_terraform_error: TerraformError::ManagedDatabaseError {
                    database_name: Some("ze73ae545-mongodb".to_string()),
                    database_type: "docdb".to_string(),
                    database_error_sub_type: Box::new(DatabaseError::VersionUpgradeNotPossible {
                        from: "4.0.0".to_string(),
                        to: "5.0.0".to_string(),
                    }),
                    raw_message: r#"Error: Failed to modify DocDB Cluster (ze73ae545-mongodb): InvalidParameterCombination: Cannot upgrade docdb from 4.0.0 to 5.0.0
        status code: 400, request id: f6f2f684-4994-45a7-a29a-b75bbb3ebb1b

  with aws_docdb_cluster.documentdb_cluster,
  on main.tf line 45, in resource "aws_docdb_cluster" "documentdb_cluster":
  45: resource "aws_docdb_cluster" "documentdb_cluster" {"#.to_string(),
                },
            },
            TestCase {
                input_raw_error: r#"Error: error creating ElastiCache Cache Cluster: InvalidParameterCombination: The combination of the cluster class 'cache.t4g.micro', cache engine 'redis' and cache engine version '5.0.6' is not supported. Please consult the documentation for supported combinations of cluster class and cache engine.
	status code: 400, request id: fe420d33-e0b1-497f-bdb8-3c656e7da6ba

  with aws_elasticache_cluster.elasticache_cluster,
  on main.tf line 29, in resource "aws_elasticache_cluster" "elasticache_cluster":
  29: resource "aws_elasticache_cluster" "elasticache_cluster" {"#,
                expected_terraform_error: TerraformError::ManagedDatabaseError {
                    database_name: None,
                    database_type: "redis".to_string(),
                    database_error_sub_type: Box::new(DatabaseError::VersionNotSupportedOnTheInstanceType {
                        version: "5.0.6".to_string(),
                        db_instance_type: "cache.t4g.micro".to_string(),
                    }),
                    raw_message: r#"Error: error creating ElastiCache Cache Cluster: InvalidParameterCombination: The combination of the cluster class 'cache.t4g.micro', cache engine 'redis' and cache engine version '5.0.6' is not supported. Please consult the documentation for supported combinations of cluster class and cache engine.
	status code: 400, request id: fe420d33-e0b1-497f-bdb8-3c656e7da6ba

  with aws_elasticache_cluster.elasticache_cluster,
  on main.tf line 29, in resource "aws_elasticache_cluster" "elasticache_cluster":
  29: resource "aws_elasticache_cluster" "elasticache_cluster" {"#.to_string(),
                },
            },
        ];

        for tc in test_cases {
            // execute:
            let result = TerraformError::new(vec!["apply".to_string()], "".to_string(), tc.input_raw_error.to_string());

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
                user: Some("arn:aws:iam::542561660426:user/thomas".to_string()),
                action: Some("iam:CreatePolicy".to_string()),
                resource_type_and_name: "policy qovery-aws-EBS-CSI-Driver-z2242cca3".to_string(),
                raw_message,
            },
            result
        );
    }

    #[test]
    fn test_terraform_error_aws_already_existing_resource_issue() {
        // setup:
        let raw_message = r#"Error: creating EKS Cluster (qovery-zd3c17088): ResourceInUseException: Cluster already exists with name: qovery-zd3c17088
{
  RespMetadata: {
    StatusCode: 409,
    RequestID: "dc9831bc-bcac-422c-8195-df7ab1219282"
  },
  ClusterName: "qovery-zd3c17088",
  Message_: "Cluster already exists with name: qovery-zd3c17088"
}

  with aws_eks_cluster.eks_cluster,
  on eks-master-cluster.tf line 35, in resource "aws_eks_cluster" "eks_cluster":
  35: resource "aws_eks_cluster" "eks_cluster" {"#.to_string();

        // execute:
        let result = TerraformError::new(vec!["apply".to_string()], "".to_string(), raw_message.to_string());

        // validate:
        assert_eq!(
            TerraformError::AlreadyExistingResource {
                resource_type: "EKS Cluster".to_string(),
                resource_name: Some("qovery-zd3c17088".to_string()),
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
    fn test_terraform_error_aws_s3_bucket_already_owned_by_you_issue() {
        // setup:
        let raw_message = r#"Unknown error while performing Terraform command (`terraform apply -no-color -auto-approve tf_plan`), here is the error:

Error: creating Amazon S3 (Simple Storage) Bucket (qovery-logs-z0bb3e862): BucketAlreadyOwnedByYou: Your previous request to create the named bucket succeeded and you already own it.
	status code: 409, request id: FMD190YF8MQ35W7F, host id: YLBPFgtZPS1V1WnXBYfQN/7BfBEW0S5KiDlgLWSCYIVWWbzPM5YNKUgP6f/Sor+jGs7FTNNEurA=

  with aws_s3_bucket.loki_bucket,
  on helm-loki.tf line 55, in resource "aws_s3_bucket" "loki_bucket":
  55: resource "aws_s3_bucket" "loki_bucket" {"#;

        // execute:
        let result = TerraformError::new(vec!["apply".to_string()], "".to_string(), raw_message.to_string());

        // validate:
        assert_eq!(
            TerraformError::S3BucketAlreadyOwnedByYou {
                bucket_name: "qovery-logs-z0bb3e862".to_string(),
                terraform_resource_name: "loki_bucket".to_string(),
                raw_message: raw_message.to_string(),
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
            },
            TestCase {
                input_raw_error: r#"Error: deleting EC2 Internet Gateway (igw-035c1695edd69cb10): error detaching EC2 Internet Gateway (igw-035c1695edd69cb10) from VPC (vpc-074b19bdada752f7e): DependencyViolation: Network vpc-074b19bdada752f7e has some mapped public address(es). Please unmap those public address(es) before detaching the gateway."#,
                expected_terraform_error: TerraformError::ResourceDependencyViolation {
                    resource_name: "igw-035c1695edd69cb10".to_string(),
                    resource_kind: "EC2 Internet Gateway".to_string(),
                    raw_message: r#"Error: deleting EC2 Internet Gateway (igw-035c1695edd69cb10): error detaching EC2 Internet Gateway (igw-035c1695edd69cb10) from VPC (vpc-074b19bdada752f7e): DependencyViolation: Network vpc-074b19bdada752f7e has some mapped public address(es). Please unmap those public address(es) before detaching the gateway."#.to_string(),
                },
            },
        ];

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

    #[test]
    fn test_terraform_error_cluster_version_unsupported_update() {
        // setup:
        let raw_terraform_error_str = r#"CreateError - Unknown error while performing Terraform command (`terraform apply -no-color -auto-approve tf_plan`), here is the error:

Error: updating EKS Cluster (qovery-z09a5408e) version: InvalidParameterException: Unsupported Kubernetes minor version update from 1.24 to 1.23
{
  RespMetadata: {
    StatusCode: 400,
    RequestID: "e8410277-627f-48e9-80b2-d2236f04ba04"
  },
  ClusterName: "qovery-z09a5408e",
  Message_: "Unsupported Kubernetes minor version update from 1.24 to 1.23"
}

  with aws_eks_cluster.eks_cluster,
  on eks-master-cluster.tf line 35, in resource "aws_eks_cluster" "eks_cluster":
  35: resource "aws_eks_cluster" "eks_cluster" {"#;

        // execute:
        let result =
            TerraformError::new(vec!["apply".to_string()], "".to_string(), raw_terraform_error_str.to_string());

        // validate:
        assert_eq!(
            TerraformError::ClusterVersionUnsupportedUpdate {
                cluster_target_version: "1.23".to_string(),
                cluster_actual_version: "1.24".to_string(),
                raw_message: raw_terraform_error_str.to_string(),
            },
            result
        );
    }

    struct DumbCommand {}

    impl ExecutableCommand for DumbCommand {
        fn get_args(&self) -> Vec<String> {
            todo!()
        }

        fn kill(&self, _cmd_handle: &mut Child) {
            todo!()
        }

        fn exec(&mut self) -> Result<(), CommandError> {
            Ok(())
        }

        fn exec_with_output<STDOUT, STDERR>(
            &mut self,
            _stdout_output: &mut STDOUT,
            _stderr_output: &mut STDERR,
        ) -> Result<(), CommandError>
        where
            STDOUT: FnMut(String),
            STDERR: FnMut(String),
        {
            Ok(())
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
            Ok(())
        }
    }

    #[cfg_attr(test, faux::create)]
    struct DumbValidator {}

    #[cfg_attr(test, faux::methods)]
    impl TerraformValidator for DumbValidator {
        fn name(&self) -> String {
            "Dumb validator".to_string()
        }

        fn description(&self) -> String {
            "A very dumb validator for testing only".to_string()
        }

        fn validate(&self, _plan_output: &TerraformOutput) -> Result<(), TerraformValidationError> {
            Ok(())
        }
    }

    #[test]
    fn test_terraform_exec_from_command_validator() {
        // setup:
        let mut validator_mock = DumbValidator::faux();

        for validator_is_valid in [true, false] {
            faux::when!(validator_mock.validate(_)).then_return(match validator_is_valid {
                true => Ok(()),
                false => Err(TerraformValidationError::HasForbiddenDestructiveChanges {
                    validator_name: "mocked".to_string(),
                    validator_description: "mocked".to_string(),
                    resource: "mocked".to_string(),
                    raw_output: "mocked".to_string(),
                }),
            });

            // execute:
            let result =
                terraform_exec_from_command(&mut DumbCommand {}, &TerraformValidators::Custom(vec![&validator_mock]));

            // validate:
            assert_eq!(validator_is_valid, result.is_ok());
        }
    }
}

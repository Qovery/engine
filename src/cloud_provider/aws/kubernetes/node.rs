use crate::cloud_provider::kubernetes::InstanceType;
use crate::errors::CommandError;
use core::fmt;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use strum_macros::EnumIter;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, EnumIter)]
pub enum AwsInstancesType {
    T2Large,    // 2 cores 8Gb RAM
    T2Xlarge,   // 4 cores 16Gb RAM
    T3Small,    // 2 cores 2Gb RAM
    T3Medium,   // 2 cores 4Gb RAM
    T3Large,    // 2 cores 8Gb RAM
    T3Xlarge,   // 4 cores 16Gb RAM
    T32xlarge,  // 8 cores 32Gb RAM
    T3aSmall,   // 2 cores 2Gb RAM
    T3aMedium,  // 2 cores 4Gb RAM
    T3aLarge,   // 2 cores 8Gb RAM
    T3aXlarge,  // 4 cores 16Gb RAM
    T3a2xlarge, // 8 cores 32Gb RAM
    M5large,    // 2 cores 8Gb RAM
    M5Xlarge,   // 4 cores 16Gb RAM
    M52Xlarge,  // 8 cores 32Gb RAM
    M54Xlarge,  // 16 cores 64Gb RAM
}

impl InstanceType for AwsInstancesType {
    fn to_cloud_provider_format(&self) -> String {
        match self {
            AwsInstancesType::T2Large => "t2.large",
            AwsInstancesType::T2Xlarge => "t2.xlarge",
            AwsInstancesType::T3Large => "t3.large",
            AwsInstancesType::T3Xlarge => "t3.xlarge",
            AwsInstancesType::T32xlarge => "t3.2xlarge",
            AwsInstancesType::T3aMedium => "t3a.medium",
            AwsInstancesType::T3aLarge => "t3a.large",
            AwsInstancesType::T3aXlarge => "t3a.xlarge",
            AwsInstancesType::T3a2xlarge => "t3a.2xlarge",
            AwsInstancesType::T3Small => "t3.small",
            AwsInstancesType::T3Medium => "t3.medium",
            AwsInstancesType::T3aSmall => "t3a.small",
            AwsInstancesType::M5large => "m5.large",
            AwsInstancesType::M5Xlarge => "m5.xlarge",
            AwsInstancesType::M52Xlarge => "m5.2xlarge",
            AwsInstancesType::M54Xlarge => "m5.4xlarge",
        }
        .to_string()
    }
}

impl AwsInstancesType {
    pub fn as_str(&self) -> &str {
        match self {
            AwsInstancesType::T2Large => "t2.large",
            AwsInstancesType::T2Xlarge => "t2.xlarge",
            AwsInstancesType::T3Large => "t3.large",
            AwsInstancesType::T3Xlarge => "t3.xlarge",
            AwsInstancesType::T32xlarge => "t3.2xlarge",
            AwsInstancesType::T3aMedium => "t3a.medium",
            AwsInstancesType::T3aLarge => "t3a.large",
            AwsInstancesType::T3aXlarge => "t3a.xlarge",
            AwsInstancesType::T3a2xlarge => "t3a.2xlarge",
            AwsInstancesType::T3Small => "t3.small",
            AwsInstancesType::T3Medium => "t3.medium",
            AwsInstancesType::T3aSmall => "t3a.small",
            AwsInstancesType::M5large => "m5.large",
            AwsInstancesType::M5Xlarge => "m5.xlarge",
            AwsInstancesType::M52Xlarge => "m5.2xlarge",
            AwsInstancesType::M54Xlarge => "m5.4xlarge",
        }
    }
}

impl fmt::Display for AwsInstancesType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            AwsInstancesType::T2Large => write!(f, "t2.large"),
            AwsInstancesType::T2Xlarge => write!(f, "t2.xlarge"),
            AwsInstancesType::T3Large => write!(f, "t3.large"),
            AwsInstancesType::T3Xlarge => write!(f, "t3.xlarge"),
            AwsInstancesType::T32xlarge => write!(f, "t3.2xlarge"),
            AwsInstancesType::T3aMedium => write!(f, "t3a.medium"),
            AwsInstancesType::T3aLarge => write!(f, "t3a.large"),
            AwsInstancesType::T3aXlarge => write!(f, "t3a.xlarge"),
            AwsInstancesType::T3a2xlarge => write!(f, "t3a.2xlarge"),
            AwsInstancesType::T3Small => write!(f, "t3.small"),
            AwsInstancesType::T3Medium => write!(f, "t3.medium"),
            AwsInstancesType::T3aSmall => write!(f, "t3a.small"),
            AwsInstancesType::M5large => write!(f, "m5.large"),
            AwsInstancesType::M5Xlarge => write!(f, "m5.xlarge"),
            AwsInstancesType::M52Xlarge => write!(f, "m5.2xlarge"),
            AwsInstancesType::M54Xlarge => write!(f, "m5.4xlarge"),
        }
    }
}

impl FromStr for AwsInstancesType {
    type Err = CommandError;

    fn from_str(s: &str) -> Result<AwsInstancesType, CommandError> {
        match s {
            "t2.large" => Ok(AwsInstancesType::T2Large),
            "t2.xlarge" => Ok(AwsInstancesType::T2Xlarge),
            "t3.large" => Ok(AwsInstancesType::T3Large),
            "t3.xlarge" => Ok(AwsInstancesType::T3Xlarge),
            "t3.2xlarge" => Ok(AwsInstancesType::T32xlarge),
            "t3a.medium" => Ok(AwsInstancesType::T3aMedium),
            "t3a.large" => Ok(AwsInstancesType::T3aLarge),
            "t3a.xlarge" => Ok(AwsInstancesType::T3aXlarge),
            "t3a.2xlarge" => Ok(AwsInstancesType::T3a2xlarge),
            "t3.small" => Ok(AwsInstancesType::T3Small),
            "t3.medium" => Ok(AwsInstancesType::T3Medium),
            "t3a.small" => Ok(AwsInstancesType::T3aSmall),
            "m5.large" => Ok(AwsInstancesType::M5large),
            "m5.xlarge" => Ok(AwsInstancesType::M5Xlarge),
            "m5.2xlarge" => Ok(AwsInstancesType::M52Xlarge),
            "m5.4xlarge" => Ok(AwsInstancesType::M54Xlarge),
            _ => Err(CommandError::new_from_safe_message(format!(
                "`{}` instance type is not supported",
                s
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::aws::kubernetes::node::AwsInstancesType;
    use crate::cloud_provider::kubernetes::InstanceType;
    use crate::cloud_provider::models::NodeGroups;
    use std::str::FromStr;
    use strum::IntoEnumIterator;

    #[test]
    fn test_aws_instance_type_to_cloud_provider_format() {
        for instance_type in AwsInstancesType::iter() {
            // verify:
            // check if instance to AWS format is the proper one for all instance types
            let result_to_string = instance_type.to_cloud_provider_format();
            assert_eq!(
                match instance_type {
                    AwsInstancesType::T2Large => "t2.large",
                    AwsInstancesType::T2Xlarge => "t2.xlarge",
                    AwsInstancesType::T3Large => "t3.large",
                    AwsInstancesType::T3Xlarge => "t3.xlarge",
                    AwsInstancesType::T32xlarge => "t3.2xlarge",
                    AwsInstancesType::T3aMedium => "t3a.medium",
                    AwsInstancesType::T3aLarge => "t3a.large",
                    AwsInstancesType::T3aXlarge => "t3a.xlarge",
                    AwsInstancesType::T3a2xlarge => "t3a.2xlarge",
                    AwsInstancesType::T3Small => "t3.small",
                    AwsInstancesType::T3Medium => "t3.medium",
                    AwsInstancesType::T3aSmall => "t3a.small",
                    AwsInstancesType::M5large => "m5.large",
                    AwsInstancesType::M5Xlarge => "m5.xlarge",
                    AwsInstancesType::M52Xlarge => "m5.2xlarge",
                    AwsInstancesType::M54Xlarge => "m5.4xlarge",
                }
                .to_string(),
                result_to_string
            );

            // then check the other way around
            match AwsInstancesType::from_str(&result_to_string) {
                Ok(result_instance_type) => assert_eq!(instance_type, result_instance_type),
                Err(_) => panic!(),
            }
        }
    }

    #[test]
    fn test_groups_nodes() {
        assert!(NodeGroups::new("".to_string(), 2, 1, "t2.large".to_string(), 20).is_err());
        assert!(NodeGroups::new("".to_string(), 2, 2, "t2.large".to_string(), 20).is_ok());
        assert!(NodeGroups::new("".to_string(), 2, 3, "t2.large".to_string(), 20).is_ok());

        assert_eq!(
            NodeGroups::new("".to_string(), 2, 2, "t2.large".to_string(), 20).unwrap(),
            NodeGroups {
                name: "".to_string(),
                id: None,
                min_nodes: 2,
                max_nodes: 2,
                instance_type: "t2.large".to_string(),
                disk_size_in_gib: 20,
                desired_nodes: None
            }
        );
    }
}

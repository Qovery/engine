use crate::cloud_provider::kubernetes::InstanceType;
use crate::errors::CommandError;
use core::fmt;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AwsInstancesType {
    T2Large,    // 2 cores 8Gb RAM
    T2Xlarge,   // 4 cores 16Gb RAM
    T3Large,    // 2 cores 8Gb RAM
    T3Xlarge,   // 4 cores 16Gb RAM
    T3aMedium,  // 2 cores 4Gb RAM
    T3aLarge,   // 2 cores 8Gb RAM
    T3a2xlarge, // 8 cores 32Gb RAM
}

impl InstanceType for AwsInstancesType {
    fn to_cloud_provider_format(&self) -> String {
        match self {
            AwsInstancesType::T2Large => "t2.large",
            AwsInstancesType::T2Xlarge => "t2.xlarge",
            AwsInstancesType::T3Large => "t3.large",
            AwsInstancesType::T3Xlarge => "t3.xlarge",
            AwsInstancesType::T3aMedium => "t3a.medium",
            AwsInstancesType::T3aLarge => "t3a.large",
            AwsInstancesType::T3a2xlarge => "t3a.2xlarge",
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
            AwsInstancesType::T3aMedium => "t3a.medium",
            AwsInstancesType::T3aLarge => "t3a.large",
            AwsInstancesType::T3a2xlarge => "t3a.2xlarge",
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
            AwsInstancesType::T3aMedium => write!(f, "t3a.medium"),
            AwsInstancesType::T3aLarge => write!(f, "t3a.large"),
            AwsInstancesType::T3a2xlarge => write!(f, "t3a.2xlarge"),
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
            "t3a.medium" => Ok(AwsInstancesType::T3aMedium),
            "t3a.large" => Ok(AwsInstancesType::T3aLarge),
            "t3a.2xlarge" => Ok(AwsInstancesType::T3a2xlarge),
            _ => Err(CommandError::new_from_safe_message(format!(
                "`{}` instance type is not supported",
                s
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::models::NodeGroups;

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
                disk_size_in_gib: 20
            }
        );
    }
}

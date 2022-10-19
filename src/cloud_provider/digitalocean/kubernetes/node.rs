use crate::cloud_provider::kubernetes::InstanceType;
use crate::errors::CommandError;
use core::fmt;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

// flavors helper: https://pcr.cloud-mercato.com/providers/flavors?provider=digitalocean
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DoInstancesType {
    S1vcpu1gb,
    S1vcpu2gb,
    S1vcpu3gb,
    S2vcpu2gb,
    S2vcpu4gb,
    S3vcpu1gb,
    S4vcpu8gb,
    S6vcpu16gb,
    S8vcpu16gb,
    S8vcpu32gb,
    S12vcpu48gb,
    S16vcpu64gb,
    S20vcpu96gb,
    S24vcpu128gb,
    S32vcpu192gb,
}

impl InstanceType for DoInstancesType {
    fn to_cloud_provider_format(&self) -> String {
        match self {
            DoInstancesType::S1vcpu1gb => "s-1vcpu-1gb",
            DoInstancesType::S1vcpu2gb => "s-1vcpu-2gb",
            DoInstancesType::S1vcpu3gb => "s-1vcpu-3gb",
            DoInstancesType::S2vcpu2gb => "s-2vcpu-2gb",
            DoInstancesType::S3vcpu1gb => "s-3vcpu-1gb",
            DoInstancesType::S2vcpu4gb => "s-2vcpu-4gb",
            DoInstancesType::S4vcpu8gb => "s-4vcpu-8gb",
            DoInstancesType::S6vcpu16gb => "s-6vcpu-16gb",
            DoInstancesType::S8vcpu16gb => "s-8vcpu-16gb",
            DoInstancesType::S8vcpu32gb => "s-8vcpu-32gb",
            DoInstancesType::S12vcpu48gb => "s-12vcpu-48gb",
            DoInstancesType::S16vcpu64gb => "s-16vcpu-64gb",
            DoInstancesType::S20vcpu96gb => "s-20vcpu-96gb",
            DoInstancesType::S24vcpu128gb => "s-24vcpu-128gb",
            DoInstancesType::S32vcpu192gb => "s-32vcpu-192gb",
        }
        .to_string()
    }

    fn is_instance_allowed(&self) -> bool {
        true
    }

    fn is_instance_cluster_allowed(&self) -> bool {
        true
    }
}

impl DoInstancesType {
    pub fn as_str(&self) -> &str {
        match self {
            DoInstancesType::S1vcpu1gb => "s-1vcpu-1gb",
            DoInstancesType::S1vcpu2gb => "s-1vcpu-2gb",
            DoInstancesType::S1vcpu3gb => "s-1vcpu-3gb",
            DoInstancesType::S2vcpu2gb => "s-2vcpu-2gb",
            DoInstancesType::S2vcpu4gb => "s-2vcpu-4gb",
            DoInstancesType::S3vcpu1gb => "s-3vcpu-1gb",
            DoInstancesType::S4vcpu8gb => "s-4vcpu-8gb",
            DoInstancesType::S6vcpu16gb => "s-6vcpu-16gb",
            DoInstancesType::S8vcpu16gb => "s-8vcpu-16gb",
            DoInstancesType::S8vcpu32gb => "s-8vcpu-32gb",
            DoInstancesType::S12vcpu48gb => "s-12vcpu-48gb",
            DoInstancesType::S16vcpu64gb => "s-16vcpu-64gb",
            DoInstancesType::S20vcpu96gb => "s-20vcpu-96gb",
            DoInstancesType::S24vcpu128gb => "s-24vcpu-128gb",
            DoInstancesType::S32vcpu192gb => "s-32vcpu-192gb",
        }
    }
}

impl fmt::Display for DoInstancesType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            DoInstancesType::S1vcpu1gb => write!(f, "s-1vcpu-1gb"),
            DoInstancesType::S1vcpu2gb => write!(f, "s-1vcpu-2gb"),
            DoInstancesType::S1vcpu3gb => write!(f, "s-1vcpu-3gb"),
            DoInstancesType::S2vcpu2gb => write!(f, "s-2vcpu-2gb"),
            DoInstancesType::S2vcpu4gb => write!(f, "s-2vcpu-4gb"),
            DoInstancesType::S3vcpu1gb => write!(f, "s-3vcpu-1gb"),
            DoInstancesType::S4vcpu8gb => write!(f, "s-4vcpu-8gb"),
            DoInstancesType::S6vcpu16gb => write!(f, "s-6vcpu-16gb"),
            DoInstancesType::S8vcpu16gb => write!(f, "s-8vcpu-16gb"),
            DoInstancesType::S8vcpu32gb => write!(f, "s-8vcpu-32gb"),
            DoInstancesType::S12vcpu48gb => write!(f, "s-12vcpu-48gb"),
            DoInstancesType::S16vcpu64gb => write!(f, "s-16vcpu-64gb"),
            DoInstancesType::S20vcpu96gb => write!(f, "s-20vcpu-96gb"),
            DoInstancesType::S24vcpu128gb => write!(f, "s-24vcpu-128gb"),
            DoInstancesType::S32vcpu192gb => write!(f, "s-32vcpu-192gb"),
        }
    }
}

impl FromStr for DoInstancesType {
    type Err = CommandError;

    fn from_str(s: &str) -> Result<DoInstancesType, CommandError> {
        match s {
            "s-1vcpu-1gb" => Ok(DoInstancesType::S1vcpu1gb),
            "s-1vcpu-2gb" => Ok(DoInstancesType::S1vcpu2gb),
            "s-1vcpu-3gb" => Ok(DoInstancesType::S1vcpu3gb),
            "s-2vcpu-2gb" => Ok(DoInstancesType::S2vcpu2gb),
            "s-2vcpu-4gb" => Ok(DoInstancesType::S2vcpu4gb),
            "s-3vcpu-1gb" => Ok(DoInstancesType::S3vcpu1gb),
            "s-4vcpu-8gb" => Ok(DoInstancesType::S4vcpu8gb),
            "s-6vcpu-16gb" => Ok(DoInstancesType::S6vcpu16gb),
            "s-8vcpu-16gb" => Ok(DoInstancesType::S8vcpu16gb),
            "s-8vcpu-32gb" => Ok(DoInstancesType::S8vcpu32gb),
            "s-12vcpu-48gb" => Ok(DoInstancesType::S12vcpu48gb),
            "s-16vcpu-64gb" => Ok(DoInstancesType::S16vcpu64gb),
            "s-20vcpu-96gb" => Ok(DoInstancesType::S20vcpu96gb),
            "s-24vcpu-128gb" => Ok(DoInstancesType::S24vcpu128gb),
            "s-32vcpu-192gb" => Ok(DoInstancesType::S32vcpu192gb),
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
        assert!(NodeGroups::new("".to_string(), 2, 1, "s-2vcpu-4gb".to_string(), 20).is_err());
        assert!(NodeGroups::new("".to_string(), 2, 2, "s-2vcpu-4gb".to_string(), 20).is_ok());
        assert!(NodeGroups::new("".to_string(), 2, 3, "s-2vcpu-4gb".to_string(), 20).is_ok());

        assert_eq!(
            NodeGroups::new("".to_string(), 2, 2, "s-2vcpu-4gb".to_string(), 20).unwrap(),
            NodeGroups {
                name: "".to_string(),
                id: None,
                min_nodes: 2,
                max_nodes: 2,
                instance_type: "s-2vcpu-4gb".to_string(),
                disk_size_in_gib: 20,
                desired_nodes: None
            }
        );
    }
}

use crate::cloud_provider::kubernetes::InstanceType;
use core::fmt;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DoInstancesType {
    S1vcpu1gb,
    S1vcpu2gb,
    S1vcpu3gb,
    S2vcpu4gb,
    S4vcpu8gb,
    S6vcpu16gb,
    S8vcpu32gb,
}

impl InstanceType for DoInstancesType {
    fn to_cloud_provider_format(&self) -> String {
        match self {
            DoInstancesType::S1vcpu1gb => "s-1vcpu-1gb",
            DoInstancesType::S1vcpu2gb => "s-1vcpu-2gb",
            DoInstancesType::S1vcpu3gb => "s-1vcpu-3gb",
            DoInstancesType::S2vcpu4gb => "s-2vcpu-4gb",
            DoInstancesType::S4vcpu8gb => "s-4vcpu-8gb",
            DoInstancesType::S6vcpu16gb => "s-6vcpu-16gb",
            DoInstancesType::S8vcpu32gb => "s-8vcpu-32gb",
        }
        .to_string()
    }
}

impl DoInstancesType {
    pub fn as_str(&self) -> &str {
        match self {
            DoInstancesType::S1vcpu1gb => "s-1vcpu-1gb",
            DoInstancesType::S1vcpu2gb => "s-1vcpu-2gb",
            DoInstancesType::S1vcpu3gb => "s-1vcpu-3gb",
            DoInstancesType::S2vcpu4gb => "s-2vcpu-4gb",
            DoInstancesType::S4vcpu8gb => "s-4vcpu-8gb",
            DoInstancesType::S6vcpu16gb => "s-6vcpu-16gb",
            DoInstancesType::S8vcpu32gb => "s-8vcpu-32gb",
        }
    }
}

impl fmt::Display for DoInstancesType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            DoInstancesType::S1vcpu1gb => write!(f, "s-1vcpu-1gb"),
            DoInstancesType::S1vcpu2gb => write!(f, "s-1vcpu-2gb"),
            DoInstancesType::S1vcpu3gb => write!(f, "s-1vcpu-3gb"),
            DoInstancesType::S2vcpu4gb => write!(f, "s-2vcpu-4gb"),
            DoInstancesType::S4vcpu8gb => write!(f, "s-4vcpu-8gb"),
            DoInstancesType::S6vcpu16gb => write!(f, "s-6vcpu-16gb"),
            DoInstancesType::S8vcpu32gb => write!(f, "s-8vcpu-32gb"),
        }
    }
}

impl FromStr for DoInstancesType {
    type Err = ();

    fn from_str(s: &str) -> Result<DoInstancesType, ()> {
        match s {
            "s-1vcpu-1gb" => Ok(DoInstancesType::S1vcpu1gb),
            "s-1vcpu-2gb" => Ok(DoInstancesType::S1vcpu2gb),
            "s-1vcpu-3gb" => Ok(DoInstancesType::S1vcpu3gb),
            "s-2vcpu-4gb" => Ok(DoInstancesType::S2vcpu4gb),
            "s-4vcpu-8gb" => Ok(DoInstancesType::S4vcpu8gb),
            "s-6vcpu-16gb" => Ok(DoInstancesType::S6vcpu16gb),
            "s-8vcpu-32gb" => Ok(DoInstancesType::S8vcpu32gb),
            _ => Err(()),
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
                min_nodes: 2,
                max_nodes: 2,
                instance_type: "s-2vcpu-4gb".to_string(),
                disk_size: 20
            }
        );
    }
}

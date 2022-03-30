use crate::cloud_provider::kubernetes::InstanceType;
use crate::errors::CommandError;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ScwInstancesType {
    Gp1Xs,   // 4 cores 16 Gb RAM
    Gp1S,    // 8 cores 32 Gb RAM
    Gp1M,    // 16 cores 64 Gb RAM
    Gp1L,    // 32 cores 128 Gb RAM
    Gp1Xl,   // 64 cores 256 Gb RAM
    Dev1M,   // 3 cores 4 Gb RAM
    Dev1L,   // 4 cores 8 Gb RAM
    Dev1Xl,  // 4 cores 12 Gb RAM
    RenderS, // 10 cores 45 Gb RAM 1 GPU 1 Gb VRAM
}

impl InstanceType for ScwInstancesType {
    fn to_cloud_provider_format(&self) -> String {
        match self {
            ScwInstancesType::Gp1Xs => "gp1-xs",
            ScwInstancesType::Gp1S => "gp1-s",
            ScwInstancesType::Gp1M => "gp1-m",
            ScwInstancesType::Gp1L => "gp1-l",
            ScwInstancesType::Gp1Xl => "gp1-xl",
            ScwInstancesType::Dev1M => "dev1-m",
            ScwInstancesType::Dev1L => "dev1-l",
            ScwInstancesType::Dev1Xl => "dev1-xl",
            ScwInstancesType::RenderS => "render-s",
        }
        .to_string()
    }
}

impl ScwInstancesType {
    pub fn as_str(&self) -> &str {
        match self {
            ScwInstancesType::Gp1Xs => "gp1-xs",
            ScwInstancesType::Gp1S => "gp1-s",
            ScwInstancesType::Gp1M => "gp1-m",
            ScwInstancesType::Gp1L => "gp1-l",
            ScwInstancesType::Gp1Xl => "gp1-xl",
            ScwInstancesType::Dev1M => "dev1-m",
            ScwInstancesType::Dev1L => "dev1-l",
            ScwInstancesType::Dev1Xl => "dev1-xl",
            ScwInstancesType::RenderS => "render-s",
        }
    }
}

impl fmt::Display for ScwInstancesType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ScwInstancesType::Gp1Xs => write!(f, "gp1-xs"),
            ScwInstancesType::Gp1S => write!(f, "gp1-s"),
            ScwInstancesType::Gp1M => write!(f, "gp1-m"),
            ScwInstancesType::Gp1L => write!(f, "gp1-l"),
            ScwInstancesType::Gp1Xl => write!(f, "gp1-xl"),
            ScwInstancesType::Dev1M => write!(f, "dev1-m"),
            ScwInstancesType::Dev1L => write!(f, "dev1-l"),
            ScwInstancesType::Dev1Xl => write!(f, "dev1-xl"),
            ScwInstancesType::RenderS => write!(f, "render-s"),
        }
    }
}

impl FromStr for ScwInstancesType {
    type Err = CommandError;

    fn from_str(s: &str) -> Result<ScwInstancesType, CommandError> {
        match s {
            "gp1-xs" => Ok(ScwInstancesType::Gp1Xs),
            "gp1-s" => Ok(ScwInstancesType::Gp1S),
            "gp1-m" => Ok(ScwInstancesType::Gp1M),
            "gp1-l" => Ok(ScwInstancesType::Gp1L),
            "gp1-xl" => Ok(ScwInstancesType::Gp1Xl),
            "dev1-m" => Ok(ScwInstancesType::Dev1M),
            "dev1-l" => Ok(ScwInstancesType::Dev1L),
            "dev1-xl" => Ok(ScwInstancesType::Dev1Xl),
            "render-s" => Ok(ScwInstancesType::RenderS),
            _ => {
                let message = format!("`{}` instance type is not supported", s);
                Err(CommandError::new(message.clone(), Some(message)))
            }
        }
    }
}

#[derive(Clone)]
pub struct ScwNodeGroup {
    pub name: String,
    pub id: Option<String>,
    pub min_nodes: i32,
    pub max_nodes: i32,
    pub instance_type: String,
    pub disk_size_in_gib: i32,
    pub status: scaleway_api_rs::models::scaleway_k8s_v1_pool::Status,
}

impl ScwNodeGroup {
    pub fn new(
        id: Option<String>,
        group_name: String,
        min_nodes: i32,
        max_nodes: i32,
        instance_type: String,
        disk_size_in_gib: i32,
        status: scaleway_api_rs::models::scaleway_k8s_v1_pool::Status,
    ) -> Result<Self, CommandError> {
        if min_nodes > max_nodes {
            let msg = format!(
                "The number of minimum nodes ({}) for group name {} is higher than maximum nodes ({})",
                &group_name, &min_nodes, &max_nodes
            );
            return Err(CommandError::new_from_safe_message(msg));
        }

        Ok(ScwNodeGroup {
            name: group_name,
            id,
            min_nodes,
            max_nodes,
            instance_type,
            disk_size_in_gib,
            status,
        })
    }
}

#[cfg(test)]
mod tests {
    #[cfg(test)]
    mod tests {
        use crate::cloud_provider::models::NodeGroups;

        #[test]
        fn test_groups_nodes() {
            assert!(NodeGroups::new("".to_string(), 2, 1, "dev1-l".to_string(), 20).is_err());
            assert!(NodeGroups::new("".to_string(), 2, 2, "dev1-l".to_string(), 20).is_ok());
            assert!(NodeGroups::new("".to_string(), 2, 3, "dev1-l".to_string(), 20).is_ok());

            assert_eq!(
                NodeGroups::new("".to_string(), 2, 2, "dev1-l".to_string(), 20).unwrap(),
                NodeGroups {
                    name: "".to_string(),
                    id: None,
                    min_nodes: 2,
                    max_nodes: 2,
                    instance_type: "dev1-l".to_string(),
                    disk_size_in_gib: 20
                }
            );
        }
    }
}

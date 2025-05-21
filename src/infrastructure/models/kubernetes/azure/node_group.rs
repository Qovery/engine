use crate::infrastructure::models::cloud_provider::azure::locations::AzureZone;
use crate::infrastructure::models::kubernetes::azure::node::AzureInstancesType;
use crate::io_models::models::CpuArchitecture;
use serde_derive::Serialize;

#[derive(Serialize, Clone)]
pub struct AzureNodeGroup {
    pub name: String,
    pub min_nodes: i32,
    pub max_nodes: i32,
    pub instance_type: AzureInstancesType,
    pub disk_size_in_gib: i32,
    pub instance_architecture: CpuArchitecture,
    pub zone: AzureZone,
}

#[derive(Serialize, Clone)]
pub struct AzureNodeGroups {
    list: Vec<AzureNodeGroup>,
}

impl AzureNodeGroups {
    pub fn new(list: Vec<AzureNodeGroup>) -> Self {
        AzureNodeGroups { list }
    }

    /// Get first node group as default one
    pub fn get_default_node_group(&self) -> Option<&AzureNodeGroup> {
        self.list.first()
    }

    /// Get additional node groups but first one
    pub fn get_additional_node_groups(&self) -> Vec<&AzureNodeGroup> {
        self.list.iter().skip(1).collect()
    }

    pub fn get_all_node_groups(&self) -> Vec<&AzureNodeGroup> {
        self.list.iter().collect()
    }
}

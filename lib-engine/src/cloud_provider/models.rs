use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Debug, Clone, Eq, PartialEq, Hash)]
pub struct EnvironmentVariable {
    pub key: String,
    pub value: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EnvironmentVariableDataTemplate {
    pub key: String,
    pub value: String,
}

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct Storage<T> {
    pub id: String,
    pub long_id: Uuid,
    pub name: String,
    pub storage_type: T,
    pub size_in_gib: u16,
    pub mount_point: String,
    pub snapshot_retention_in_days: u16,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StorageDataTemplate {
    pub id: String,
    pub long_id: Uuid,
    pub name: String,
    pub storage_type: String,
    pub size_in_gib: u16,
    pub mount_point: String,
    pub snapshot_retention_in_days: u16,
}

#[derive(Clone, Debug)]
pub struct CustomDomain {
    pub domain: String,
    pub target_domain: String,
}

#[derive(Serialize, Deserialize)]
pub struct CustomDomainDataTemplate {
    pub domain: String,
    pub domain_hash: String,
    pub target_domain: String,
}

pub struct Route {
    pub path: String,
    pub service_long_id: Uuid,
}

#[derive(Serialize, Deserialize)]
pub struct RouteDataTemplate {
    pub path: String,
    pub application_name: String,
    pub application_port: u16,
}

#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct CpuLimits {
    pub cpu_request: String,
    pub cpu_limit: String,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct NodeGroups {
    pub name: String,
    pub id: Option<String>,
    pub min_nodes: i32,
    pub max_nodes: i32,
    pub desired_nodes: Option<i32>,
    pub instance_type: String,
    pub disk_size_in_gib: i32,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct NodeGroupsWithDesiredState {
    pub name: String,
    pub id: Option<String>,
    pub min_nodes: i32,
    pub max_nodes: i32,
    pub desired_size: i32,
    pub enable_desired_size: bool,
    pub instance_type: String,
    pub disk_size_in_gib: i32,
}

#[derive(Serialize, Deserialize)]
pub struct NodeGroupsFormat {
    pub name: String,
    pub min_nodes: String,
    pub max_nodes: String,
    pub instance_type: String,
    pub disk_size_in_gib: String,
}

pub struct InstanceEc2 {
    pub instance_type: String,
    pub disk_size_in_gib: i32,
}

#[derive(Debug, Copy, Clone)]
pub enum KubernetesClusterAction {
    Bootstrap,
    Update(Option<i32>),
    Upgrade(Option<i32>),
    Pause,
    Resume(Option<i32>),
    Delete,
}

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct WorkerNodeDataTemplate {
    pub instance_type: String,
    pub desired_size: String,
    pub max_size: String,
    pub min_size: String,
}

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct EnvironmentVariable {
    pub key: String,
    pub value: String,
}

#[derive(Serialize, Deserialize)]
pub struct EnvironmentVariableDataTemplate {
    pub key: String,
    pub value: String,
}

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct Storage<T> {
    pub id: String,
    pub name: String,
    pub storage_type: T,
    pub size_in_gib: u16,
    pub mount_point: String,
    pub snapshot_retention_in_days: u16,
}

#[derive(Serialize, Deserialize)]
pub struct StorageDataTemplate {
    pub id: String,
    pub name: String,
    pub storage_type: String,
    pub size_in_gib: u16,
    pub mount_point: String,
    pub snapshot_retention_in_days: u16,
}

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
    pub application_name: String,
}

#[derive(Serialize, Deserialize)]
pub struct RouteDataTemplate {
    pub path: String,
    pub application_name: String,
    pub application_port: u16,
}

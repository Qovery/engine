use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
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
}

#[derive(Serialize)]
pub struct HostDataTemplate {
    pub domain_name: String,
    pub service_name: String,
    pub service_port: u16,
}

pub struct Route {
    pub path: String,
    pub service_long_id: Uuid,
}

//
#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct CpuLimits {
    pub cpu_request: String, // TODO(benjaminch): Replace String by KubernetesCpuResourceUnit to leverage conversion and type
    pub cpu_limit: String, // TODO(benjaminch): Replace String by KubernetesCpuResourceUnit to leverage conversion and type
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone)]
pub struct NodeGroups {
    pub name: String,
    pub id: Option<String>,
    pub min_nodes: i32,
    pub max_nodes: i32,
    pub desired_nodes: Option<i32>,
    pub instance_type: String,
    pub disk_size_in_gib: i32,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone)]
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

/// Represents Kubernetes CPU resource unit
/// https://kubernetes.io/docs/concepts/configuration/manage-resources-containers/#meaning-of-cpu
///
/// TODO(benjaminch): Implement From<String> for KubernetesCpuResourceUnit
pub enum KubernetesCpuResourceUnit {
    /// Milli CPU
    MilliCpu(u32),
}

impl Display for KubernetesCpuResourceUnit {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            match &self {
                KubernetesCpuResourceUnit::MilliCpu(v) => format!("{}m", v),
            }
            .as_str(),
        )
    }
}

/// Represents Kubernetes memory resource unit
/// https://kubernetes.io/docs/concepts/configuration/manage-resources-containers/#meaning-of-memory
///
/// TODO(benjaminch): Implement From<String> for KubernetesMemoryResourceUnit
pub enum KubernetesMemoryResourceUnit {
    /// MebiByte: 1 Mebibyte (MiB) = (1024)^2 bytes = 1,048,576 bytes.
    MebiByte(u32),
    /// MegaByte: 1 Megabyte (MB) = (1000)^2 bytes = 1,000,000 bytes.
    MegaByte(u32),
    /// GibiByte: 1 Gibibyte (MiB) = 2^30 bytes bytes = 1,073,741,824 bytes.
    GibiByte(u32),
    /// GigaByte: 1 Gigabyte (G) = 10^9 bytes = 1,000,000,000 bytes
    GigaByte(u32),
}

impl Display for KubernetesMemoryResourceUnit {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            match &self {
                KubernetesMemoryResourceUnit::MebiByte(v) => format!("{}Mi", v),
                KubernetesMemoryResourceUnit::MegaByte(v) => format!("{}M", v),
                KubernetesMemoryResourceUnit::GibiByte(v) => format!("{}Gi", v),
                KubernetesMemoryResourceUnit::GigaByte(v) => format!("{}G", v),
            }
            .as_str(),
        )
    }
}

pub trait IngressLoadBalancerType {
    fn annotation_key(&self) -> String;
    fn annotation_value(&self) -> String;
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};

    #[test]
    fn test_kubernetes_cpu_resource_unit_to_string() {
        // setup:
        struct TestCase<'a> {
            input: KubernetesCpuResourceUnit,
            output: &'a str,
        }

        let test_cases = vec![
            TestCase {
                input: KubernetesCpuResourceUnit::MilliCpu(0),
                output: "0m",
            },
            TestCase {
                input: KubernetesCpuResourceUnit::MilliCpu(100),
                output: "100m",
            },
        ];

        for tc in test_cases {
            // execute & verify:
            assert_eq!(tc.output, tc.input.to_string());
        }
    }

    #[test]
    fn test_kubernetes_memory_resource_unit_to_string() {
        // setup:
        struct TestCase<'a> {
            input: KubernetesMemoryResourceUnit,
            output: &'a str,
        }

        let test_cases = vec![
            TestCase {
                input: KubernetesMemoryResourceUnit::MebiByte(0),
                output: "0Mi",
            },
            TestCase {
                input: KubernetesMemoryResourceUnit::MebiByte(100),
                output: "100Mi",
            },
            TestCase {
                input: KubernetesMemoryResourceUnit::MegaByte(0),
                output: "0M",
            },
            TestCase {
                input: KubernetesMemoryResourceUnit::MegaByte(100),
                output: "100M",
            },
            TestCase {
                input: KubernetesMemoryResourceUnit::GibiByte(0),
                output: "0Gi",
            },
            TestCase {
                input: KubernetesMemoryResourceUnit::GibiByte(100),
                output: "100Gi",
            },
            TestCase {
                input: KubernetesMemoryResourceUnit::GigaByte(0),
                output: "0G",
            },
            TestCase {
                input: KubernetesMemoryResourceUnit::GigaByte(100),
                output: "100G",
            },
        ];

        for tc in test_cases {
            // execute & verify:
            assert_eq!(tc.output, tc.input.to_string());
        }
    }
}

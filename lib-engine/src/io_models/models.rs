use crate::infrastructure::models::cloud_provider::service::ServiceType;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;
use uuid::Uuid;

use crate::helm::ChartValuesGenerated;

#[derive(Serialize, Debug, Clone, Eq, PartialEq, Hash)]
pub struct EnvironmentVariable {
    pub key: String,
    pub value: String,
    pub is_secret: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EnvironmentVariableDataTemplate {
    pub key: String,
    pub value: String,
}

#[derive(Serialize, Clone, Debug, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct MountedFile {
    pub id: String,
    pub long_id: Uuid,
    pub mount_path: String,
    pub file_content_b64: String,
}

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct StorageClass(pub String);

impl Display for StorageClass {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct Storage {
    pub id: String,
    pub long_id: Uuid,
    pub name: String,
    pub storage_class: StorageClass,
    pub size_in_gib: u32,
    pub mount_point: String,
    pub snapshot_retention_in_days: u16,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StorageDataTemplate {
    pub id: String,
    pub long_id: Uuid,
    pub name: String,
    pub storage_type: String,
    pub size_in_gib: u32,
    pub mount_point: String,
    pub snapshot_retention_in_days: u16,
}

#[derive(Clone, Debug)]
pub struct CustomDomain {
    pub domain: String,
    pub target_domain: String,
    pub generate_certificate: bool,
    pub use_cdn: bool,
}
impl CustomDomain {
    const WILDCARD_PREFIX: &'static str = "*.";

    pub fn is_wildcard(&self) -> bool {
        self.domain.starts_with(Self::WILDCARD_PREFIX)
    }

    pub fn domain_without_wildcard(&self) -> &str {
        self.domain.strip_prefix(Self::WILDCARD_PREFIX).unwrap_or(&self.domain)
    }
}

#[derive(Serialize, Deserialize, Eq, PartialEq)]
pub struct CustomDomainDataTemplate {
    pub domain: String,
}

#[derive(Serialize, Eq, PartialEq)]
pub struct KubeService {
    pub namespace_key: Option<String>,
    pub name: String,
    pub ports: Vec<KubeServicePort>,
    pub selectors: BTreeMap<String, String>,
}

#[derive(Serialize, Eq, PartialEq)]
pub struct KubeServicePort {
    pub port: u16,
    pub target_port: u16,
    pub protocol: String,
}

#[derive(Serialize, Eq, PartialEq)]
pub struct HostDataTemplate {
    pub domain_name: String,
    pub service_name: String,
    pub service_port: u16,
}

pub struct Route {
    pub path: String,
    pub service_long_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VpcQoveryNetworkMode {
    WithNatGateways,
    WithoutNatGateways,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpcCustomRoutingTable {
    description: String,
    destination: String,
    target: String,
}

impl fmt::Display for VpcQoveryNetworkMode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
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
    pub instance_architecture: CpuArchitecture,
    pub zone: Option<String>,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone, Copy, Hash)]
pub enum CpuArchitecture {
    AMD64,
    ARM64,
}

impl Display for CpuArchitecture {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            CpuArchitecture::AMD64 => write!(f, "AMD64"),
            CpuArchitecture::ARM64 => write!(f, "ARM64"),
        }
    }
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
    pub instance_architecture: CpuArchitecture,
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
    pub instance_architecture: CpuArchitecture,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum KubernetesClusterAction {
    Bootstrap,
    Update(Option<i32>),
    Upgrade(Option<i32>),
    Pause,
    Resume(Option<i32>),
    Delete,
    CleanKarpenterMigration,
}

#[derive(Debug, Clone)]
pub struct InvalidStatefulsetStorage {
    pub service_type: ServiceType,
    pub service_id: Uuid,
    pub statefulset_selector: String,
    pub statefulset_name: String,
    pub invalid_pvcs: Vec<InvalidPVCStorage>,
}

#[derive(Debug, Clone)]
pub struct InvalidPVCStorage {
    pub pvc_name: String,
    pub required_disk_size_in_gib: u32,
}

pub static KUBERNETES_CPU_RESOURCE_VALUE_REGEX: Lazy<Regex> = Lazy::new(|| {
    let pattern = r"^(\d+)(m)$";
    Regex::new(pattern).unwrap()
});

/// Represents Kubernetes CPU resource unit
/// https://kubernetes.io/docs/concepts/configuration/manage-resources-containers/#meaning-of-cpu
#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub enum KubernetesCpuResourceUnit {
    /// Milli CPU
    MilliCpu(u32),
}

impl From<KubernetesCpuResourceUnit> for u32 {
    fn from(value: KubernetesCpuResourceUnit) -> u32 {
        match value {
            KubernetesCpuResourceUnit::MilliCpu(v) => v,
        }
    }
}

impl FromStr for KubernetesCpuResourceUnit {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let cpu_value_with_unit = match KUBERNETES_CPU_RESOURCE_VALUE_REGEX.captures(s) {
            None => return Err(format!("Cannot get KubernetesCpuResourceUnit from string '{s}'")),
            Some(capture) => capture,
        };

        let cpu_size = match cpu_value_with_unit[1].parse::<u32>() {
            Ok(cpu_size) => cpu_size,
            Err(err) => return Err(format!("Cannot parse cpu size part: {err}")),
        };

        let unit = &cpu_value_with_unit[2];
        let kubernetes_cpu_resource_unit = match unit {
            "m" => KubernetesCpuResourceUnit::MilliCpu(cpu_size),
            _ => return Err(format!("Unsupported cpu unit found: '{unit}' (only Mi,Gi,M,G are supported)")),
        };

        Ok(kubernetes_cpu_resource_unit)
    }
}

impl Display for KubernetesCpuResourceUnit {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(
            match &self {
                KubernetesCpuResourceUnit::MilliCpu(v) => format!("{v}m"),
            }
            .as_str(),
        )
    }
}

pub static KUBERNETES_MEMORY_RESOURCE_VALUE_REGEX: Lazy<Regex> = Lazy::new(|| {
    let pattern = r"^(\d+)(Mi|Gi|M|G)$";
    Regex::new(pattern).unwrap()
});

/// Represents Kubernetes memory resource unit
/// https://kubernetes.io/docs/concepts/configuration/manage-resources-containers/#meaning-of-memory
#[derive(Serialize, Deserialize, Clone, Debug, Eq, PartialEq)]
pub enum KubernetesMemoryResourceUnit {
    /// MebiByte: 1 Mebibyte (Mi) = (1024)^2 bytes = 1,048,576 bytes.
    MebiByte(u32),
    /// MegaByte: 1 Megabyte (M) = (1000)^2 bytes = 1,000,000 bytes.
    MegaByte(u32),
    /// GibiByte: 1 Gibibyte (Gi) = 2^30 bytes bytes = 1,073,741,824 bytes.
    GibiByte(u32),
    /// GigaByte: 1 Gigabyte (G) = 10^9 bytes = 1,000,000,000 bytes
    GigaByte(u32),
}

impl Display for KubernetesMemoryResourceUnit {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(
            match &self {
                KubernetesMemoryResourceUnit::MebiByte(v) => format!("{v}Mi"),
                KubernetesMemoryResourceUnit::MegaByte(v) => format!("{v}M"),
                KubernetesMemoryResourceUnit::GibiByte(v) => format!("{v}Gi"),
                KubernetesMemoryResourceUnit::GigaByte(v) => format!("{v}G"),
            }
            .as_str(),
        )
    }
}

impl FromStr for KubernetesMemoryResourceUnit {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let memory_value_with_unit = match KUBERNETES_MEMORY_RESOURCE_VALUE_REGEX.captures(s) {
            None => return Err(format!("Cannot get KubernetesMemoryResourceUnit from string '{s}'")),
            Some(capture) => capture,
        };

        let memory_size = match memory_value_with_unit[1].parse::<u32>() {
            Ok(memory_size) => memory_size,
            Err(err) => return Err(format!("Cannot parse memory size part: {err}")),
        };

        let unit = &memory_value_with_unit[2];
        let kubernetes_memory_resource_unit = match unit {
            "Mi" => KubernetesMemoryResourceUnit::MebiByte(memory_size),
            "Gi" => KubernetesMemoryResourceUnit::GibiByte(memory_size),
            "M" => KubernetesMemoryResourceUnit::MegaByte(memory_size),
            "G" => KubernetesMemoryResourceUnit::GigaByte(memory_size),
            _ => {
                return Err(format!(
                    "Unsupported memory unit found: '{unit}' (only Mi,Gi,M,G are supported)"
                ));
            }
        };

        Ok(kubernetes_memory_resource_unit)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CustomerHelmChartsOverride {
    pub chart_name: String,
    pub chart_values: String,
}

impl CustomerHelmChartsOverride {
    pub fn to_chart_values_generated(&self) -> ChartValuesGenerated {
        ChartValuesGenerated {
            filename: format!("customer_{}_override.yaml", self.chart_name),
            yaml_content: self.chart_values.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::io_models::models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};
    use serde::Deserialize;
    use serde_derive::Serialize;
    use serde_with::DisplayFromStr;
    use std::str::FromStr;

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
    fn should_get_kubernetes_cpu_unit_from_string() {
        // setup:
        struct TestCase<'a> {
            input: &'a str,
            output: KubernetesCpuResourceUnit,
        }

        let test_cases = vec![
            TestCase {
                input: "0m",
                output: KubernetesCpuResourceUnit::MilliCpu(0),
            },
            TestCase {
                input: "100m",
                output: KubernetesCpuResourceUnit::MilliCpu(100),
            },
        ];

        for tc in test_cases {
            // execute & verify:
            assert_eq!(
                tc.output,
                KubernetesCpuResourceUnit::from_str(tc.input)
                    .unwrap_or_else(|_| panic!("{} failed to be computed", tc.input))
            );
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

    #[test]
    fn should_get_kubernetes_memory_unit_from_string() {
        // given
        // setup:
        struct TestCase<'a> {
            input: &'a str,
            output: KubernetesMemoryResourceUnit,
        }

        let test_cases = vec![
            TestCase {
                input: "0Mi",
                output: KubernetesMemoryResourceUnit::MebiByte(0),
            },
            TestCase {
                input: "100Mi",
                output: KubernetesMemoryResourceUnit::MebiByte(100),
            },
            TestCase {
                input: "0M",
                output: KubernetesMemoryResourceUnit::MegaByte(0),
            },
            TestCase {
                input: "100M",
                output: KubernetesMemoryResourceUnit::MegaByte(100),
            },
            TestCase {
                input: "0Gi",
                output: KubernetesMemoryResourceUnit::GibiByte(0),
            },
            TestCase {
                input: "100Gi",
                output: KubernetesMemoryResourceUnit::GibiByte(100),
            },
            TestCase {
                input: "0G",
                output: KubernetesMemoryResourceUnit::GigaByte(0),
            },
            TestCase {
                input: "100G",
                output: KubernetesMemoryResourceUnit::GigaByte(100),
            },
        ];

        // when
        for tc in test_cases {
            // execute & verify:
            assert_eq!(
                tc.output,
                KubernetesMemoryResourceUnit::from_str(tc.input)
                    .unwrap_or_else(|_| panic!("{} failed to be computed", tc.input))
            );
        }
    }

    #[test]
    fn should_deserialize_kubernetes_units() {
        // given
        #[serde_with::serde_as]
        #[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
        struct DeserializeTarget {
            #[serde_as(as = "DisplayFromStr")]
            pub memory_in_gi: KubernetesMemoryResourceUnit,
            #[serde_as(as = "DisplayFromStr")]
            pub memory_in_mi: KubernetesMemoryResourceUnit,
            #[serde_as(as = "DisplayFromStr")]
            pub memory_in_g: KubernetesMemoryResourceUnit,
            #[serde_as(as = "DisplayFromStr")]
            pub memory_in_m: KubernetesMemoryResourceUnit,
            #[serde_as(as = "DisplayFromStr")]
            pub cpu_in_m: KubernetesCpuResourceUnit,
        }

        let json = r#"
        {
           "memory_in_gi": "10Gi",
           "memory_in_mi": "20Mi",
           "memory_in_g": "30G",
           "memory_in_m": "40M",
           "cpu_in_m": "1000m"
        }
        "#;

        // when
        let result = serde_json::from_str::<DeserializeTarget>(json);

        // then
        assert!(result.is_ok());
        let deserialize_target = result.expect("Should be Ok");
        assert_eq!(
            deserialize_target,
            DeserializeTarget {
                memory_in_gi: KubernetesMemoryResourceUnit::GibiByte(10),
                memory_in_mi: KubernetesMemoryResourceUnit::MebiByte(20),
                memory_in_g: KubernetesMemoryResourceUnit::GigaByte(30),
                memory_in_m: KubernetesMemoryResourceUnit::MegaByte(40),
                cpu_in_m: KubernetesCpuResourceUnit::MilliCpu(1000),
            }
        );
    }
}

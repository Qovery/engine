use crate::environment::models::domain::ToTerraformString;
use crate::helm::ChartValuesGenerated;
use crate::infrastructure::models::cloud_provider::service::ServiceType;
use crate::infrastructure::models::kubernetes::disk::{DiskIops, DiskThroughput};
use crate::infrastructure::models::kubernetes::scaleway::scaleway_public_gateway_type::ScalewayPublicGatewayType;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::{self, Display, Formatter};
use std::str::FromStr;
use uuid::Uuid;

#[derive(Serialize, Debug, Clone, Eq, PartialEq, Hash)]
pub struct EnvironmentVariable {
    pub key: String,
    pub value: String,
    pub is_secret: bool,
}

/// Raw deserialization struct for an external secret entry from the deployment JSON.
#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
pub struct ExternalSecret {
    /// Path/name of the secret in the remote secret manager (e.g. "/prod/db/credentials").
    pub external_secret_name: String,
    /// UUID of the ClusterSecretStore to target (`store-{UUID}`).
    pub secret_manager_access_id: Uuid,
    /// If set, the secret is mounted as a file at this path instead of being injected as an env var.
    pub mount_path: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EnvironmentVariableDataTemplate {
    pub key: String,
    pub value: String,
}

#[derive(Serialize, Clone, Debug, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct MountedFile {
    pub long_id: Uuid,
    pub kube_name: String,
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

    /// Returns true if this wildcard domain matches `domain` the way DNS resolves a wildcard.
    /// A wildcard matches exactly one label, so `*.toto.com` covers `tata.toto.com` but neither
    /// `http.tata.toto.com`, `*.tata.toto.com`, nor the apex `toto.com` itself.
    pub fn covers(&self, domain: &str) -> bool {
        // a wildcard is a name to request on its own, it is never covered by another wildcard
        if !self.is_wildcard() || domain.starts_with(Self::WILDCARD_PREFIX) {
            return false;
        }

        // dns names are case insensitive, and custom domains reach us verbatim from the customer
        let domain = domain.to_ascii_lowercase();
        let parent_domain = self.domain_without_wildcard().to_ascii_lowercase();

        let Some(label) = domain
            .strip_suffix(parent_domain.as_str())
            .and_then(|label| label.strip_suffix('.'))
        else {
            return false;
        };

        !label.is_empty() && !label.contains('.')
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

#[derive(Serialize, Clone, Debug, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub enum HostPathType {
    Exact,
    PathPrefix,
    RegularExpression,
}

impl HostPathType {
    pub fn from_path(path: &str, default_host_path_type: HostPathType) -> HostPathType {
        let has_character_class = path.contains('[') && path.contains(']');
        let has_quantifier = path.contains('*') || path.contains('+') || path.contains('?');
        let has_alternation = path.contains('|');
        let has_group = path.contains('(') && path.contains(')');
        let has_anchor = path.starts_with('^') || path.ends_with('$');

        let has_wildcard_pattern = path.contains(".*") || path.contains(".+");

        if has_character_class || has_quantifier || has_alternation || has_group || has_anchor || has_wildcard_pattern {
            HostPathType::RegularExpression
        } else if path == "/" || path.ends_with('/') {
            HostPathType::PathPrefix
        } else {
            default_host_path_type
        }
    }
}

#[derive(Serialize, Clone, Debug, Eq, PartialEq)]
pub struct HostDataTemplate {
    pub domain_name: String,
    pub service_name: String,
    pub service_port: u16,
    pub path: String,
    pub path_rewrite: Option<String>,
    pub path_type: HostPathType,
    pub weight: u32,
}

pub struct Route {
    pub path: String,
    pub service_long_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum VpcQoveryNetworkMode {
    WithoutNatGateways,
    WithNatGateways,
}

impl Display for VpcQoveryNetworkMode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                VpcQoveryNetworkMode::WithoutNatGateways => "WithoutNatGateways".to_string(),
                VpcQoveryNetworkMode::WithNatGateways => "WithNatGateways".to_string(),
            }
        )
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone)]
#[serde(tag = "provider")]
#[serde(rename_all = "lowercase")]
pub enum NatGatewayType {
    Scaleway {
        #[serde(rename = "type")]
        nat_gateway_type: ScalewayPublicGatewayType,
    },
    Gcp {
        #[serde(default, alias = "staticIpsEnabled")]
        static_ips_enabled: bool,
        #[serde(default, alias = "staticIpsCount")]
        static_ips_count: Option<u16>,
    },
}

impl Display for NatGatewayType {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            NatGatewayType::Scaleway { nat_gateway_type } => write!(f, "{nat_gateway_type}"),
            NatGatewayType::Gcp { .. } => write!(f, "gcp"),
        }
    }
}

impl ToTerraformString for NatGatewayType {
    fn to_terraform_format_string(&self) -> String {
        self.to_string()
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone, Default)]
pub struct NatGatewayParameters {
    #[serde(default, alias = "natGatewayType")]
    pub nat_gateway_type: Option<NatGatewayType>,
}

impl NatGatewayParameters {
    pub fn scaleway_nat_gateway_type(&self) -> Option<&NatGatewayType> {
        match self.nat_gateway_type.as_ref() {
            Some(NatGatewayType::Scaleway { .. }) => self.nat_gateway_type.as_ref(),
            _ => None,
        }
    }

    pub fn gcp_static_ips_enabled(&self) -> bool {
        match self.nat_gateway_type.as_ref() {
            Some(NatGatewayType::Gcp { static_ips_enabled, .. }) => *static_ips_enabled,
            _ => false,
        }
    }

    pub fn gcp_static_ips_count(&self) -> Option<u16> {
        match self.nat_gateway_type.as_ref() {
            Some(NatGatewayType::Gcp { static_ips_count, .. }) => *static_ips_count,
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpcCustomRoutingTable {
    description: String,
    destination: String,
    target: String,
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
    pub min_nodes: u32,
    pub max_nodes: u32,
    pub desired_nodes: Option<u32>,
    pub instance_type: String,
    pub disk_size_in_gib: u32,
    pub disk_iops: Option<DiskIops>,
    pub disk_throughput: Option<DiskThroughput>,
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
    pub min_nodes: u32,
    pub max_nodes: u32,
    pub desired_size: u32,
    pub enable_desired_size: bool,
    pub instance_type: String,
    pub disk_size_in_gib: u32,
    pub disk_iops: Option<DiskIops>,
    pub disk_throughput: Option<DiskThroughput>,
    pub instance_architecture: CpuArchitecture,
}

#[derive(Serialize, Deserialize)]
pub struct NodeGroupsFormat {
    pub name: String,
    pub min_nodes: String,
    pub max_nodes: String,
    pub instance_type: String,
    pub disk_size_in_gib: String,
    pub disk_iops: Option<String>,
    pub disk_throughput: Option<String>,
}

pub struct InstanceEc2 {
    pub instance_type: String,
    pub disk_size_in_gib: u32,
    pub instance_architecture: CpuArchitecture,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum KubernetesClusterAction {
    Bootstrap,
    Update(Option<u32>),
    Upgrade(Option<u32>),
    Pause,
    Resume(Option<u32>),
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

impl KubernetesCpuResourceUnit {
    pub fn to_millicpu(&self) -> u32 {
        match self {
            KubernetesCpuResourceUnit::MilliCpu(v) => *v,
        }
    }
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
    /// GibiByte: 1 Gibibyte (Gi) = 2^30 bytes = 1,073,741,824 bytes.
    GibiByte(u32),
    /// GigaByte: 1 Gigabyte (G) = 10^9 bytes = 1,000,000,000 bytes
    GigaByte(u32),
}

impl KubernetesMemoryResourceUnit {
    pub fn to_mebibyte(&self) -> u32 {
        match self {
            KubernetesMemoryResourceUnit::MebiByte(v) => *v,
            KubernetesMemoryResourceUnit::MegaByte(v) => (*v as f64 / 1.049).ceil() as u32,
            KubernetesMemoryResourceUnit::GibiByte(v) => *v * 1024,
            KubernetesMemoryResourceUnit::GigaByte(v) => (*v as f64 * 0.954).ceil() as u32,
        }
    }
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

/// Represents Kubernetes GPU resource unit
/// https://kubernetes.io/docs/tasks/manage-gpus/scheduling-gpus/
#[derive(Serialize, Deserialize, Copy, Clone, Debug, Eq, PartialEq)]
pub struct KubernetesGpuResourceUnit(pub u32);

impl KubernetesGpuResourceUnit {
    pub fn to_gpu_count(&self) -> u32 {
        self.0
    }
}

impl From<KubernetesGpuResourceUnit> for u32 {
    fn from(value: KubernetesGpuResourceUnit) -> u32 {
        value.0
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
    use crate::environment::models::domain::ToTerraformString;
    use crate::infrastructure::models::kubernetes::scaleway::scaleway_public_gateway_type::ScalewayPublicGatewayType;
    use crate::io_models::models::{
        CustomDomain, HostPathType, KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit, NatGatewayParameters,
        NatGatewayType,
    };
    use serde::Deserialize;
    use serde_derive::Serialize;
    use serde_with::DisplayFromStr;
    use std::str::FromStr;
    use strum::IntoEnumIterator;

    #[test]
    fn test_custom_domain_covers() {
        // setup:
        struct TestCase<'a> {
            wildcard: &'a str,
            domain: &'a str,
            expected: bool,
            description: &'a str,
        }

        let test_cases = vec![
            TestCase {
                wildcard: "*.toto.com",
                domain: "tata.toto.com",
                expected: true,
                description: "direct subdomain is covered",
            },
            TestCase {
                wildcard: "*.toto.com",
                domain: "toto.com",
                expected: false,
                description: "a wildcard never covers its own apex",
            },
            TestCase {
                wildcard: "*.toto.com",
                domain: "http.tata.toto.com",
                expected: false,
                description: "a wildcard matches a single label only",
            },
            TestCase {
                wildcard: "*.toto.com",
                domain: "*.tata.toto.com",
                expected: false,
                description: "a wildcard does not cover a nested wildcard",
            },
            TestCase {
                wildcard: "*.toto.com",
                domain: "nottoto.com",
                expected: false,
                description: "lookalike suffix is not covered",
            },
            TestCase {
                wildcard: "*.toto.com",
                domain: "com",
                expected: false,
                description: "parent of the wildcard is not covered",
            },
            TestCase {
                wildcard: "*.toto.com",
                domain: "*.toto.com",
                expected: false,
                description: "a wildcard does not cover itself",
            },
            TestCase {
                wildcard: "toto.com",
                domain: "tata.toto.com",
                expected: false,
                description: "a non wildcard domain covers nothing",
            },
            TestCase {
                wildcard: "*.Toto.com",
                domain: "tata.TOTO.com",
                expected: true,
                description: "dns names are case insensitive",
            },
            TestCase {
                wildcard: "*.TOTO.com",
                domain: "TOTO.com",
                expected: false,
                description: "apex is still not covered whatever the case",
            },
        ];

        for tc in test_cases {
            // execute:
            let custom_domain = CustomDomain {
                domain: tc.wildcard.to_string(),
                target_domain: "".to_string(),
                generate_certificate: true,
                use_cdn: false,
            };

            // verify:
            assert_eq!(
                tc.expected,
                custom_domain.covers(tc.domain),
                "case {} : '{}' covers '{}'",
                tc.description,
                tc.wildcard,
                tc.domain
            );
        }
    }

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

    #[test]
    fn test_nat_gateway_parameters_deserialization_with_alias() {
        // setup (Scaleway top-level payload)
        let test_cases = vec![
            (
                r#"{"nat_gateway_type":{"provider":"scaleway","type":"VPC-GW-S"}}"#,
                ScalewayPublicGatewayType::Small,
            ),
            (
                r#"{"nat_gateway_type":{"provider":"scaleway","type":"VPC-GW-M"}}"#,
                ScalewayPublicGatewayType::Medium,
            ),
            (
                r#"{"nat_gateway_type":{"provider":"scaleway","type":"VPC-GW-L"}}"#,
                ScalewayPublicGatewayType::Large,
            ),
            (
                r#"{"nat_gateway_type":{"provider":"scaleway","type":"VPC-GW-XL"}}"#,
                ScalewayPublicGatewayType::XLarge,
            ),
        ];

        for (json, expected_type) in test_cases {
            // execute
            let result: Result<NatGatewayParameters, _> = serde_json::from_str(json);

            // verify
            assert!(result.is_ok(), "Failed to deserialize with alias: {:?}", result.err());
            let nat_gateway = result.unwrap();
            assert_eq!(
                nat_gateway.scaleway_nat_gateway_type(),
                Some(&NatGatewayType::Scaleway {
                    nat_gateway_type: expected_type
                }),
                "Failed for JSON with alias: {json}",
            );
            assert_eq!(nat_gateway.gcp_static_ips_count(), None);
        }
    }

    #[test]
    fn test_nat_gateway_parameters_roundtrip_serialization() {
        // setup
        let test_cases = vec![
            NatGatewayParameters {
                nat_gateway_type: Some(NatGatewayType::Scaleway {
                    nat_gateway_type: ScalewayPublicGatewayType::Medium,
                }),
            },
            NatGatewayParameters {
                nat_gateway_type: Some(NatGatewayType::Gcp {
                    static_ips_enabled: true,
                    static_ips_count: Some(3),
                }),
            },
        ];

        for original in test_cases {
            // execute: serialize then deserialize
            let json = serde_json::to_string(&original).expect("Failed to serialize");
            let deserialized: NatGatewayParameters = serde_json::from_str(&json).expect("Failed to deserialize");

            // verify
            assert_eq!(original, deserialized);
        }
    }

    #[test]
    fn test_nat_gateway_parameters_deserialization_invalid_provider() {
        // setup
        let json = r#"{"nat_gateway_type":{"provider":"aws","type":"some-type"}}"#;

        // execute
        let result: Result<NatGatewayParameters, _> = serde_json::from_str(json);

        // verify - should fail because "aws" is not a valid provider
        assert!(result.is_err(), "Should fail with invalid provider");
    }

    #[test]
    fn test_nat_gateway_parameters_deserialization_invalid_type() {
        // setup
        let json = r#"{"nat_gateway_type":{"provider":"scaleway","type":"InvalidType"}}"#;

        // execute
        let result: Result<NatGatewayParameters, _> = serde_json::from_str(json);

        // verify - should fail because "InvalidType" is not a valid gateway type
        assert!(result.is_err(), "Should fail with invalid gateway type");
    }

    #[test]
    fn test_nat_gateway_parameters_deserialization_static_ips_enabled_only() {
        // setup (GCP payload under nat_gateway_type)
        let json = r#"{"nat_gateway_type":{"provider":"gcp","static_ips_enabled":true}}"#;

        // execute
        let result: Result<NatGatewayParameters, _> = serde_json::from_str(json);

        // verify
        assert!(result.is_ok(), "Should parse static_ips_enabled without nat_gateway_type");
        let nat_gateway = result.unwrap();
        assert_eq!(nat_gateway.scaleway_nat_gateway_type(), None);
        assert!(nat_gateway.gcp_static_ips_enabled());
        assert_eq!(nat_gateway.gcp_static_ips_count(), None);
    }

    #[test]
    fn test_nat_gateway_parameters_deserialization_static_ips_count() {
        // setup (GCP payload with camelCase field aliases)
        let json = r#"{"nat_gateway_type":{"provider":"gcp","staticIpsEnabled":true,"staticIpsCount":1}}"#;

        // execute
        let result: Result<NatGatewayParameters, _> = serde_json::from_str(json);

        // verify
        assert!(result.is_ok(), "Should parse staticIpsCount");
        let nat_gateway = result.unwrap();
        assert!(nat_gateway.gcp_static_ips_enabled());
        assert_eq!(nat_gateway.gcp_static_ips_count(), Some(1));
    }

    #[test]
    fn test_nat_gateway_parameters_deserialization_provider_specific_shape() {
        // setup (new payload shape)
        let json = r#"{
          "nat_gateway_type":{"provider":"scaleway","type":"VPC-GW-L"},
          "ignored_extra_field":{"static_ips_enabled":true,"static_ips_count":2}
        }"#;

        // execute
        let result: Result<NatGatewayParameters, _> = serde_json::from_str(json);

        // verify
        assert!(result.is_ok(), "Should parse provider-specific shape");
        let nat_gateway = result.unwrap();
        assert_eq!(
            nat_gateway.scaleway_nat_gateway_type(),
            Some(&NatGatewayType::Scaleway {
                nat_gateway_type: ScalewayPublicGatewayType::Large
            })
        );
        assert!(!nat_gateway.gcp_static_ips_enabled());
        assert_eq!(nat_gateway.gcp_static_ips_count(), None);
    }

    #[test]
    fn test_nat_gateway_parameters_deserialization_gcp_payload_under_nat_gateway_type() {
        // setup (backward compatibility with core payload shape)
        let json = r#"{
          "nat_gateway_type":{"provider":"gcp","static_ips_enabled":false,"static_ips_count":2}
        }"#;

        // execute
        let result: Result<NatGatewayParameters, _> = serde_json::from_str(json);

        // verify
        assert!(result.is_ok(), "Should parse gcp payload under nat_gateway_type");
        let nat_gateway = result.unwrap();
        assert_eq!(nat_gateway.scaleway_nat_gateway_type(), None);
        assert!(!nat_gateway.gcp_static_ips_enabled());
        assert_eq!(nat_gateway.gcp_static_ips_count(), Some(2));
    }

    #[test]
    fn test_nat_gateway_type_to_string() {
        // setup
        let test_cases = vec![
            (
                NatGatewayType::Scaleway {
                    nat_gateway_type: ScalewayPublicGatewayType::Small,
                },
                "VPC-GW-S",
            ),
            (
                NatGatewayType::Scaleway {
                    nat_gateway_type: ScalewayPublicGatewayType::Medium,
                },
                "VPC-GW-M",
            ),
            (
                NatGatewayType::Scaleway {
                    nat_gateway_type: ScalewayPublicGatewayType::Large,
                },
                "VPC-GW-L",
            ),
            (
                NatGatewayType::Scaleway {
                    nat_gateway_type: ScalewayPublicGatewayType::XLarge,
                },
                "VPC-GW-XL",
            ),
        ];

        for (gateway_type, expected) in test_cases {
            // execute
            let result = gateway_type.to_string();

            // verify
            assert_eq!(result, expected, "Failed for gateway type: {gateway_type:?}");
        }
    }

    #[test]
    fn test_nat_gateway_type_to_terraform_format_string() {
        // setup
        let test_cases = vec![
            (
                NatGatewayType::Scaleway {
                    nat_gateway_type: ScalewayPublicGatewayType::Small,
                },
                "VPC-GW-S",
            ),
            (
                NatGatewayType::Scaleway {
                    nat_gateway_type: ScalewayPublicGatewayType::Medium,
                },
                "VPC-GW-M",
            ),
            (
                NatGatewayType::Scaleway {
                    nat_gateway_type: ScalewayPublicGatewayType::Large,
                },
                "VPC-GW-L",
            ),
            (
                NatGatewayType::Scaleway {
                    nat_gateway_type: ScalewayPublicGatewayType::XLarge,
                },
                "VPC-GW-XL",
            ),
        ];

        for (gateway_type, expected) in test_cases {
            // execute
            let result = gateway_type.to_terraform_format_string();

            // verify
            assert_eq!(result, expected, "Failed for gateway type: {gateway_type:?}");
        }
    }

    #[test]
    fn test_nat_gateway_type_display_and_terraform_format_match() {
        // setup
        for scaleway_gateway_type in ScalewayPublicGatewayType::iter() {
            // execute
            let nat_gateway_type = NatGatewayType::Scaleway {
                nat_gateway_type: scaleway_gateway_type.clone(),
            };
            let display_result = nat_gateway_type.to_string();
            let terraform_result = nat_gateway_type.to_terraform_format_string();

            // verify
            assert_eq!(
                display_result, terraform_result,
                "Display and ToTerraformString should match for {scaleway_gateway_type:?}",
            );
        }
    }

    #[test]
    fn test_nat_gateway_type_display_format() {
        // setup
        use crate::infrastructure::models::kubernetes::scaleway::scaleway_public_gateway_type::ScalewayPublicGatewayType;
        use crate::io_models::models::NatGatewayType;

        let nat_gateway_type = NatGatewayType::Scaleway {
            nat_gateway_type: ScalewayPublicGatewayType::Medium,
        };

        // execute
        let display_result = format!("{nat_gateway_type}");
        let debug_result = format!("{nat_gateway_type:?}");

        // verify
        assert_eq!(display_result, "VPC-GW-M");
        assert!(debug_result.contains("Scaleway"));
        assert!(debug_result.contains("Medium"));
    }

    #[test]
    fn test_host_path_type_from_path_returns_regular_expression_for_regex_patterns() {
        // setup
        let cases = vec![".*", "[a-z]", "path|other", "(group)", "^start$"];

        // execute
        for path in &cases {
            let result = HostPathType::from_path(path, HostPathType::Exact);

            // verify
            assert_eq!(result, HostPathType::RegularExpression, "Failed for path: {path}",);
        }
    }

    #[test]
    fn test_host_path_type_from_path_returns_prefix_for_slash_or_trailing_slash() {
        // setup
        let cases = vec!["/", "/path/"];

        // execute
        for path in &cases {
            let result = HostPathType::from_path(path, HostPathType::Exact);

            // verify
            assert_eq!(result, HostPathType::PathPrefix, "Failed for path: {path}",);
        }
    }

    #[test]
    fn test_host_path_type_from_path_returns_default_for_non_special_paths() {
        // setup
        let case_exact = ("/simple/path", HostPathType::Exact);
        let case_prefix = ("plainpath", HostPathType::PathPrefix);

        // execute
        let result_exact = HostPathType::from_path(case_exact.0, case_exact.1.clone());
        let result_prefix = HostPathType::from_path(case_prefix.0, case_prefix.1.clone());

        // verify
        assert_eq!(result_exact, HostPathType::Exact, "Failed for path: {}", case_exact.0);
        assert_eq!(result_prefix, HostPathType::PathPrefix, "Failed for path: {}", case_prefix.0);
    }

    #[test]
    fn test_host_path_type_from_path_handles_empty_path() {
        // setup
        let path = "";
        let default = HostPathType::Exact;

        // execute
        let result = HostPathType::from_path(path, default);

        // verify
        assert_eq!(
            result,
            HostPathType::Exact,
            "Empty path should return the default host path type"
        );
    }

    #[test]
    fn test_host_path_type_from_path_handles_edge_cases() {
        // setup
        let case_prefix = ("no/special/characters", HostPathType::PathPrefix);
        let case_regex = ("ends/with$", HostPathType::Exact);

        // execute
        let result_prefix = HostPathType::from_path(case_prefix.0, case_prefix.1.clone());
        let result_regex = HostPathType::from_path(case_regex.0, case_regex.1.clone());

        // verify
        assert_eq!(result_prefix, HostPathType::PathPrefix, "Failed for path: {}", case_prefix.0);
        assert_eq!(
            result_regex,
            HostPathType::RegularExpression,
            "Failed for path: {}",
            case_regex.0
        );
    }
}

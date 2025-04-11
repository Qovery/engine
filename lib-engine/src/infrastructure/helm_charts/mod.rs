use serde::Serialize;

use crate::helm::{CommonChart, HelmChartError, VpaContainerPolicy};
use crate::infrastructure::models::kubernetes::{Kind as KubernetesKind, Kind};
use crate::io_models::models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};
use std::env;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use time::Duration;

pub mod cert_manager_chart;
pub mod cert_manager_config_chart;
pub mod coredns_config_chart;
pub mod external_dns_chart;
pub mod grafana_chart;
pub mod k8s_event_logger;
pub mod kube_prometheus_stack_chart;
pub mod kube_state_metrics;
pub mod loki_chart;
pub mod metrics_server_chart;
pub mod nginx_ingress_chart;
pub mod prometheus_adapter_chart;
pub mod prometheus_operator_crds;
pub mod promtail_chart;
pub mod qovery_cert_manager_webhook_chart;
pub mod qovery_cluster_agent_chart;
pub mod qovery_priority_class_chart;
pub mod qovery_shell_agent_chart;
pub mod qovery_storage_class_chart;
pub mod thanos;
pub mod vertical_pod_autoscaler;

pub enum HelmChartTimeout {
    /// Let helm chart defines what it wants
    ChartDefault,
    /// Let user define what they want
    Custom(Duration),
}

pub enum HelmChartResourcesConstraintType {
    /// Let helm chart defines what it wants
    ChartDefault,
    /// Let user define what they want
    Constrained(HelmChartResources),
}

/// Represents Helm chart resources such as:
/// resources:
//   limits:
//     cpu: [limit_cpu_m]
//     memory: [limit_memory_mi]
//   requests:
//     cpu: [request_cpu_m]
//     memory: [request_memory_mi]
#[derive(Serialize)]
pub struct HelmChartResources {
    pub limit_cpu: KubernetesCpuResourceUnit,
    pub limit_memory: KubernetesMemoryResourceUnit,
    pub request_cpu: KubernetesCpuResourceUnit,
    pub request_memory: KubernetesMemoryResourceUnit,
}

pub struct HelmChartAutoscaling {
    pub min_replicas: u32,
    pub max_replicas: u32,
    pub target_cpu_utilization_percentage: u32,
}

pub enum HelmChartVpaType {
    /// VPA won't be enabled for the chart
    Disabled,
    /// VPA will be enabled for the chart with default values
    EnabledWithChartDefault,
    /// VPA will be enabled for the chart with custom values
    EnabledWithConstraints(VpaContainerPolicy),
}

#[derive(Clone)]
pub struct HelmChartPath {
    path: HelmPath,
}

impl HelmChartPath {
    pub fn new(
        path_prefix: Option<&str>,
        directory_location: HelmChartDirectoryLocation,
        chart_name: String,
    ) -> HelmChartPath {
        HelmChartPath {
            path: HelmPath::new(HelmPathType::Chart, path_prefix, directory_location, chart_name),
        }
    }

    pub fn helm_path(&self) -> &HelmPath {
        &self.path
    }
}

impl Display for HelmChartPath {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.path.to_string().as_str())
    }
}

#[derive(Clone)]
pub struct HelmChartValuesFilePath {
    path: HelmPath,
}

impl HelmChartValuesFilePath {
    pub fn new(
        path_prefix: Option<&str>,
        directory_location: HelmChartDirectoryLocation,
        chart_name: String,
    ) -> HelmChartValuesFilePath {
        HelmChartValuesFilePath {
            path: HelmPath::new(HelmPathType::ValuesFile, path_prefix, directory_location, chart_name),
        }
    }

    pub fn helm_path(&self) -> &HelmPath {
        &self.path
    }
}

impl Display for HelmChartValuesFilePath {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.path.to_string().as_str())
    }
}

#[derive(Clone)]
pub struct HelmChartCRDsPath {
    path: PathBuf,
}

impl HelmChartCRDsPath {
    pub fn new(helm_chart_path: HelmChartPath, crds_path: &str) -> HelmChartCRDsPath {
        HelmChartCRDsPath {
            path: PathBuf::from(format!("{}/{}", helm_chart_path.helm_path(), crds_path)),
        }
    }

    pub fn helm_path(&self) -> PathBuf {
        self.path.clone()
    }
}

impl Display for HelmChartCRDsPath {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.path.display().to_string().as_str())
    }
}

pub enum HelmPathType {
    ValuesFile,
    Chart,
}

/// Represents chart directory where chart is defined.
#[derive(Clone, Default)]
pub struct HelmPath {
    path: String,
}

impl HelmPath {
    pub fn new(
        helm_path_type: HelmPathType,
        path_prefix: Option<&str>,
        directory_location: HelmChartDirectoryLocation,
        chart_name: String,
    ) -> HelmPath {
        let mut path = format!(
            "{prefix}{directory}/{helm_path_type}/{name}{extension}",
            prefix = path_prefix.unwrap_or("."),
            directory = match directory_location {
                HelmChartDirectoryLocation::CommonFolder => "/common",
                HelmChartDirectoryLocation::CloudProviderFolder => "/",
            },
            helm_path_type = match helm_path_type {
                HelmPathType::ValuesFile => "chart_values",
                HelmPathType::Chart => "charts",
            },
            name = chart_name,
            extension = match helm_path_type {
                HelmPathType::ValuesFile => ".yaml",
                HelmPathType::Chart => "",
            }
        );

        // TODO(benjaminch: Find a more elegant way to remove consecutives /.
        while path.contains("//") {
            path = path.replace("//", "/");
        }

        HelmPath { path }
    }
}

impl Display for HelmPath {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.path.as_str())
    }
}

/// Represents where chart is supposed to be taken from (specific for provider or shared).
pub enum HelmChartDirectoryLocation {
    CommonFolder,
    CloudProviderFolder,
}

pub trait ToCommonHelmChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError>;
}

pub fn get_helm_values_set_in_code_but_absent_in_values_file(
    chart_common_chart: CommonChart,
    values_file_lib_path: String,
) -> Option<Vec<String>> {
    let current_directory = env::current_dir().expect("Impossible to get current directory");
    let chart_values_path = format!(
        "{}/{}",
        current_directory
            .to_str()
            .expect("Impossible to convert current directory to string"),
        values_file_lib_path,
    );

    let f = std::fs::File::open(&chart_values_path)
        .unwrap_or_else(|_| panic!("Impossible to open chart values file: `{chart_values_path}`"));
    let data: serde_yaml::Value =
        serde_yaml::from_reader(f).unwrap_or_else(|_| panic!("Impossible to parse YAML file: `{chart_values_path}`"));

    let mut missing_fields = vec![];

    for value in chart_common_chart.chart_info.values {
        // Check that value declared in rust code exists in the YAML values file
        if let serde_yaml::Value::Mapping(ref m) = data {
            // Black magic allowing to keep only fields before array indexes
            let fields_raw: String = value
                .key
                .to_string()
                .chars()
                .take_while(|&ch| ch != '[')
                .collect::<String>();
            let mut fields = fields_raw.split('.').map(|s| s.to_string()).collect::<Vec<String>>();

            // Since 'annotations' and 'labels' are objects with unpredictable and weird keys we only check if object is set but not what's in it.
            let mut index = 0;
            while index < fields.len() {
                if fields[index].to_lowercase().ends_with("annotations")
                    || fields[index].to_lowercase().ends_with("labels")
                {
                    fields = fields[..index + 1].to_vec();
                    break;
                }
                index += 1;
            }

            let fields_len = fields.len();

            let mut current_value = m;

            for (i, f) in fields.iter().enumerate() {
                if !current_value.contains_key(f) {
                    missing_fields.push(value.key.to_string());
                }

                if i < fields_len - 1 {
                    current_value = match current_value.get(f) {
                        Some(v) => v.as_mapping().expect("Error while trying to get nested field"),
                        None => panic!("Missing key/value '{}' in file '{}'", value.key, chart_values_path),
                    }
                }
            }
        }
    }

    match missing_fields.is_empty() {
        true => None,
        false => Some(missing_fields),
    }
}

pub enum HelmChartType {
    Shared,
    CloudProviderSpecific(KubernetesKind),
}

/// Returns helm sub path for a given chart defining if it stands in common VS cloud-provider folder.
pub fn get_helm_path_kubernetes_provider_sub_folder_name(helm_path: &HelmPath, chart_type: HelmChartType) -> String {
    let helm_chart_location = helm_path.to_string();

    match chart_type {
        HelmChartType::Shared => {
            match &helm_chart_location.contains("/common/") {
                true => "common",
                false => "undefined-cloud-provider", // There is something weird
            }
        }
        HelmChartType::CloudProviderSpecific(provider_kind) => match &helm_chart_location.contains("/common/") {
            false => match provider_kind {
                KubernetesKind::Eks | Kind::EksSelfManaged => "aws",
                KubernetesKind::ScwKapsule | Kind::ScwSelfManaged => "scaleway",
                KubernetesKind::Gke | Kind::GkeSelfManaged => "gcp",
                KubernetesKind::Aks | Kind::AksSelfManaged => "azure",
                Kind::OnPremiseSelfManaged => "on-premise",
            },
            true => "undefined-cloud-provider", // There is something weird
        },
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::models::kubernetes::Kind as KubernetesKind;

    #[test]
    fn test_helm_chart_path_to_string() {
        // setup:
        struct TestCase {
            input: HelmChartPath,
            expected_path: String,
        }

        let test_cases = vec![
            TestCase {
                input: HelmChartPath::new(None, HelmChartDirectoryLocation::CommonFolder, "yolo".to_string()),
                expected_path: "./common/charts/yolo".to_string(),
            },
            TestCase {
                input: HelmChartPath::new(
                    Some("/tmp/1234567/"),
                    HelmChartDirectoryLocation::CommonFolder,
                    "yolo".to_string(),
                ),
                expected_path: "/tmp/1234567/common/charts/yolo".to_string(),
            },
            TestCase {
                input: HelmChartPath::new(
                    Some("/tmp/1234567"),
                    HelmChartDirectoryLocation::CommonFolder,
                    "yolo".to_string(),
                ),
                expected_path: "/tmp/1234567/common/charts/yolo".to_string(),
            },
            TestCase {
                input: HelmChartPath::new(None, HelmChartDirectoryLocation::CloudProviderFolder, "yolo".to_string()),
                expected_path: "./charts/yolo".to_string(),
            },
            TestCase {
                input: HelmChartPath::new(
                    Some("/tmp/79087349856/"),
                    HelmChartDirectoryLocation::CloudProviderFolder,
                    "yolo".to_string(),
                ),
                expected_path: "/tmp/79087349856/charts/yolo".to_string(),
            },
            TestCase {
                input: HelmChartPath::new(
                    Some("/tmp/79087349856"),
                    HelmChartDirectoryLocation::CloudProviderFolder,
                    "yolo".to_string(),
                ),
                expected_path: "/tmp/79087349856/charts/yolo".to_string(),
            },
            TestCase {
                input: HelmChartPath::new(
                    Some("/tmp/////79087349856/"),
                    HelmChartDirectoryLocation::CloudProviderFolder,
                    "yolo".to_string(),
                ),
                expected_path: "/tmp/79087349856/charts/yolo".to_string(),
            },
            TestCase {
                input: HelmChartPath::new(
                    Some("./"),
                    HelmChartDirectoryLocation::CloudProviderFolder,
                    "yolo".to_string(),
                ),
                expected_path: "./charts/yolo".to_string(),
            },
            TestCase {
                input: HelmChartPath::new(
                    Some("."),
                    HelmChartDirectoryLocation::CloudProviderFolder,
                    "yolo".to_string(),
                ),
                expected_path: "./charts/yolo".to_string(),
            },
        ];

        for tc in test_cases {
            // execute:
            let res = tc.input.to_string();

            // verify:
            assert_eq!(tc.expected_path, res)
        }
    }

    #[test]
    fn test_get_helm_path_kubernetes_provider_sub_folder_name() {
        // setup:
        struct TestCase {
            helm_path_input: HelmPath,
            chart_type_input: HelmChartType,
            expected_sub_folder: String,
        }

        let test_cases = vec![
            TestCase {
                helm_path_input: HelmPath::new(
                    HelmPathType::Chart,
                    None,
                    HelmChartDirectoryLocation::CommonFolder,
                    "whatever".to_string(),
                ),
                chart_type_input: HelmChartType::Shared,
                expected_sub_folder: "common".to_string(),
            },
            TestCase {
                helm_path_input: HelmPath::new(
                    HelmPathType::Chart,
                    None,
                    HelmChartDirectoryLocation::CloudProviderFolder,
                    "whatever".to_string(),
                ),
                chart_type_input: HelmChartType::CloudProviderSpecific(KubernetesKind::Eks),
                expected_sub_folder: "aws".to_string(),
            },
            TestCase {
                helm_path_input: HelmPath::new(
                    HelmPathType::Chart,
                    None,
                    HelmChartDirectoryLocation::CloudProviderFolder,
                    "whatever".to_string(),
                ),
                chart_type_input: HelmChartType::CloudProviderSpecific(KubernetesKind::ScwKapsule),
                expected_sub_folder: "scaleway".to_string(),
            },
            // Wrongly configured
            TestCase {
                helm_path_input: HelmPath::new(
                    HelmPathType::Chart,
                    None,
                    HelmChartDirectoryLocation::CloudProviderFolder,
                    "whatever".to_string(),
                ),
                chart_type_input: HelmChartType::Shared,
                expected_sub_folder: "undefined-cloud-provider".to_string(),
            },
            TestCase {
                helm_path_input: HelmPath::new(
                    HelmPathType::Chart,
                    None,
                    HelmChartDirectoryLocation::CommonFolder,
                    "whatever".to_string(),
                ),
                chart_type_input: HelmChartType::CloudProviderSpecific(KubernetesKind::ScwKapsule),
                expected_sub_folder: "undefined-cloud-provider".to_string(),
            },
        ];

        for tc in test_cases {
            // execute:
            let res = get_helm_path_kubernetes_provider_sub_folder_name(&tc.helm_path_input, tc.chart_type_input);

            // verify:
            assert_eq!(tc.expected_sub_folder, res);
        }
    }
}

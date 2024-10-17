use std::sync::Arc;

use crate::cloud_provider::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, CommonChartVpa, HelmChartError,
    HelmChartNamespaces, PriorityClass, VpaConfig, VpaContainerPolicy, VpaTargetRef, VpaTargetRefApiVersion,
    VpaTargetRefKind,
};
use crate::cloud_provider::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::cloud_provider::models::{
    CustomerHelmChartsOverride, KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit,
};
use crate::errors::CommandError;
use kube::Client;
use semver::Version;

pub struct PromtailChart {
    chart_prefix_path: Option<String>,
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    loki_kube_dns_name: String,
    customer_helm_chart_override: Option<CustomerHelmChartsOverride>,
    enable_vpa: bool,
    namespace: HelmChartNamespaces,
    priority_class: PriorityClass,
    additional_char_path: Option<HelmChartValuesFilePath>,
}

impl PromtailChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        chart_values_location: HelmChartDirectoryLocation,
        loki_kube_dns_name: String,
        customer_helm_chart_fn: Arc<dyn Fn(String) -> Option<CustomerHelmChartsOverride>>,
        enable_vpa: bool,
        namespace: HelmChartNamespaces,
        priority_class: PriorityClass,
        karpenter_enabled: bool,
    ) -> Self {
        PromtailChart {
            chart_prefix_path: chart_prefix_path.map(|s| s.to_string()),
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                PromtailChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                chart_values_location,
                PromtailChart::chart_name(),
            ),
            loki_kube_dns_name,
            customer_helm_chart_override: customer_helm_chart_fn(Self::chart_name()),
            enable_vpa,
            namespace,
            priority_class,
            additional_char_path: match karpenter_enabled {
                true => Some(HelmChartValuesFilePath::new(
                    chart_prefix_path,
                    HelmChartDirectoryLocation::CommonFolder,
                    "promtail_with_karpenter".to_string(),
                )),
                false => None,
            },
        }
    }

    pub fn chart_name() -> String {
        "promtail".to_string()
    }
}

impl ToCommonHelmChart for PromtailChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        let mut values_files = vec![self.chart_values_path.to_string()];
        if let Some(additional_char_path) = &self.additional_char_path {
            values_files.push(additional_char_path.to_string());
        }

        let mut chart_info = ChartInfo {
            name: PromtailChart::chart_name(),
            reinstall_chart_if_installed_version_is_below_than: Some(Version::new(5, 1, 0)),
            path: self.chart_path.to_string(),
            namespace: self.namespace,
            values_files,
            values: vec![
                ChartSetValue {
                    key: "image.registry".to_string(),
                    value: "public.ecr.aws".to_string(),
                },
                ChartSetValue {
                    key: "image.repository".to_string(),
                    value: "r3m4q3r9/pub-mirror-promtail".to_string(),
                },
                ChartSetValue {
                    key: "config.clients[0].url".to_string(),
                    value: format!("http://{}/loki/api/v1/push", self.loki_kube_dns_name),
                },
            ],
            yaml_files_content: match self.customer_helm_chart_override.clone() {
                Some(x) => vec![x.to_chart_values_generated()],
                None => vec![],
            },
            // As promtail is on every node, it can take some time and failing the chart deployment
            // e.g papershift production cluster has 33 nodes !
            timeout_in_seconds: 1800,
            ..Default::default()
        };

        // Set custom priority class if provided
        if let PriorityClass::Qovery(priority_class) = &self.priority_class {
            chart_info.values.push(ChartSetValue {
                key: "priorityClassName".to_string(),
                value: priority_class.to_string(),
            });
        }

        Ok(CommonChart {
            chart_info,
            chart_installation_checker: Some(Box::new(PromtailChartChecker::new())),
            vertical_pod_autoscaler: match self.enable_vpa {
                true => Some(CommonChartVpa::new(
                    self.chart_prefix_path.clone().unwrap_or(".".to_string()),
                    vec![VpaConfig {
                        target_ref: VpaTargetRef::new(
                            VpaTargetRefApiVersion::AppsV1,
                            VpaTargetRefKind::DaemonSet,
                            "promtail".to_string(),
                        ),
                        container_policy: VpaContainerPolicy::new(
                            "*".to_string(),
                            Some(KubernetesCpuResourceUnit::MilliCpu(50)),
                            Some(KubernetesCpuResourceUnit::MilliCpu(750)),
                            Some(KubernetesMemoryResourceUnit::MebiByte(32)),
                            Some(KubernetesMemoryResourceUnit::GibiByte(1)),
                        ),
                    }],
                )),
                false => None,
            },
        })
    }
}

#[derive(Clone)]
pub struct PromtailChartChecker {}

impl PromtailChartChecker {
    pub fn new() -> PromtailChartChecker {
        PromtailChartChecker {}
    }
}

impl Default for PromtailChartChecker {
    fn default() -> Self {
        PromtailChartChecker::new()
    }
}

impl ChartInstallationChecker for PromtailChartChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO(ENG-1370): Implement chart install verification
        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::helm::{HelmChartNamespaces, PriorityClass};
    use crate::cloud_provider::helm_charts::promtail_chart::PromtailChart;
    use crate::cloud_provider::helm_charts::{
        get_helm_path_kubernetes_provider_sub_folder_name, get_helm_values_set_in_code_but_absent_in_values_file,
        HelmChartDirectoryLocation, HelmChartType, ToCommonHelmChart,
    };
    use crate::cloud_provider::models::CustomerHelmChartsOverride;
    use std::env;
    use std::sync::Arc;

    fn get_promtail_chart_override() -> Arc<dyn Fn(String) -> Option<CustomerHelmChartsOverride>> {
        Arc::new(|_chart_name: String| -> Option<CustomerHelmChartsOverride> {
            Some(CustomerHelmChartsOverride {
                chart_name: PromtailChart::chart_name(),
                chart_values: "".to_string(),
            })
        })
    }

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn promtail_chart_directory_exists_test() {
        // setup:
        let chart = PromtailChart::new(
            None,
            HelmChartDirectoryLocation::CommonFolder,
            "whatever".to_string(),
            get_promtail_chart_override(),
            false,
            HelmChartNamespaces::KubeSystem,
            PriorityClass::Default,
            false,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared),
            PromtailChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn promtail_chart_values_file_exists_test() {
        // setup:
        let chart = PromtailChart::new(
            None,
            HelmChartDirectoryLocation::CommonFolder,
            "whatever".to_string(),
            get_promtail_chart_override(),
            false,
            HelmChartNamespaces::KubeSystem,
            PriorityClass::Default,
            false,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_values_path = format!(
            "{}/lib/{}/bootstrap/chart_values/{}.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart.chart_values_path.helm_path(),
                HelmChartType::Shared
            ),
            PromtailChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
    }

    /// Make sure rust code doesn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn promtail_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = PromtailChart::new(
            None,
            HelmChartDirectoryLocation::CommonFolder,
            "whatever".to_string(),
            get_promtail_chart_override(),
            false,
            HelmChartNamespaces::KubeSystem,
            PriorityClass::Default,
            false,
        );
        let common_chart = chart.to_common_helm_chart().unwrap();

        // execute:
        let missing_fields = get_helm_values_set_in_code_but_absent_in_values_file(
            common_chart,
            format!(
                "/lib/{}/bootstrap/chart_values/{}.yaml",
                get_helm_path_kubernetes_provider_sub_folder_name(
                    chart.chart_values_path.helm_path(),
                    HelmChartType::Shared
                ),
                PromtailChart::chart_name(),
            ),
        );

        // verify:
        assert!(missing_fields.is_none(), "Some fields are missing in values file, add those (make sure they still exist in chart values), fields: {}", missing_fields.unwrap_or_default().join(","));
    }
}

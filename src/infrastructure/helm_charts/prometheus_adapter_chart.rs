use std::sync::Arc;

use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, CommonChartVpa, HelmAction, HelmChartError,
    HelmChartNamespaces, VpaConfig, VpaContainerPolicy, VpaTargetRef, VpaTargetRefApiVersion, VpaTargetRefKind,
};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::io_models::models::{CustomerHelmChartsOverride, KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};
use kube::Client;
use semver::Version;

pub struct PrometheusAdapterChart {
    action: HelmAction,
    chart_prefix_path: Option<String>,
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    prometheus_internal_url: String,
    prometheus_namespace: HelmChartNamespaces,
    customer_helm_chart_override: Option<CustomerHelmChartsOverride>,
    enable_vpa: bool,
    additional_char_path: Option<HelmChartValuesFilePath>,
}

impl PrometheusAdapterChart {
    pub fn new(
        action: HelmAction,
        chart_prefix_path: Option<&str>,
        prometheus_url: String,
        prometheus_namespace: HelmChartNamespaces,
        customer_helm_chart_fn: Arc<dyn Fn(String) -> Option<CustomerHelmChartsOverride>>,
        enable_vpa: bool,
        karpenter_enabled: bool,
    ) -> Self {
        PrometheusAdapterChart {
            action,
            chart_prefix_path: chart_prefix_path.map(|s| s.to_string()),
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                PrometheusAdapterChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                PrometheusAdapterChart::chart_name(),
            ),
            prometheus_internal_url: prometheus_url,
            prometheus_namespace,
            customer_helm_chart_override: customer_helm_chart_fn(Self::chart_name()),
            enable_vpa,
            additional_char_path: match karpenter_enabled {
                true => Some(HelmChartValuesFilePath::new(
                    chart_prefix_path,
                    HelmChartDirectoryLocation::CommonFolder,
                    "prometheus-adapter-with-karpenter".to_string(),
                )),
                false => None,
            },
        }
    }

    pub fn chart_name() -> String {
        "prometheus-adapter".to_string()
    }
}

impl ToCommonHelmChart for PrometheusAdapterChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        let mut values_files = vec![self.chart_values_path.to_string()];
        if let Some(additional_char_path) = &self.additional_char_path {
            values_files.push(additional_char_path.to_string());
        }

        Ok(CommonChart {
            chart_info: ChartInfo {
                action: self.action.clone(),
                name: "prometheus-adapter".to_string(),
                path: self.chart_path.to_string(),
                reinstall_chart_if_installed_version_is_below_than: Some(Version::new(3, 3, 1)),
                namespace: self.prometheus_namespace,
                values_files,
                values: vec![ChartSetValue {
                    key: "prometheus.url".to_string(),
                    value: self.prometheus_internal_url.clone(),
                }],
                yaml_files_content: match self.customer_helm_chart_override.clone() {
                    Some(x) => vec![x.to_chart_values_generated()],
                    None => vec![],
                },
                ..Default::default()
            },
            vertical_pod_autoscaler: match self.enable_vpa {
                true => Some(CommonChartVpa::new(
                    self.chart_prefix_path.clone().unwrap_or(".".to_string()),
                    vec![VpaConfig {
                        target_ref: VpaTargetRef::new(
                            VpaTargetRefApiVersion::AppsV1,
                            VpaTargetRefKind::Deployment,
                            "prometheus-adapter".to_string(),
                        ),
                        container_policy: VpaContainerPolicy::new(
                            "*".to_string(),
                            Some(KubernetesCpuResourceUnit::MilliCpu(250)),
                            Some(KubernetesCpuResourceUnit::MilliCpu(1000)),
                            Some(KubernetesMemoryResourceUnit::MebiByte(64)),
                            Some(KubernetesMemoryResourceUnit::GibiByte(1)),
                        ),
                    }],
                )),
                false => None,
            },
            chart_installation_checker: None,
        })
    }
}

#[derive(Clone)]
pub struct PrometheusAdapterChartChecker {}

impl PrometheusAdapterChartChecker {
    pub fn new() -> PrometheusAdapterChartChecker {
        PrometheusAdapterChartChecker {}
    }
}

impl Default for PrometheusAdapterChartChecker {
    fn default() -> Self {
        PrometheusAdapterChartChecker::new()
    }
}

impl ChartInstallationChecker for PrometheusAdapterChartChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO(ENG-1385): Implement chart install verification
        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::helm::{HelmAction, HelmChartNamespaces};
    use crate::infrastructure::helm_charts::prometheus_adapter_chart::PrometheusAdapterChart;
    use crate::infrastructure::helm_charts::{
        HelmChartType, ToCommonHelmChart, get_helm_path_kubernetes_provider_sub_folder_name,
        get_helm_values_set_in_code_but_absent_in_values_file,
    };
    use crate::io_models::models::CustomerHelmChartsOverride;
    use std::env;
    use std::sync::Arc;

    fn get_prometheus_adapter_chart_override() -> Arc<dyn Fn(String) -> Option<CustomerHelmChartsOverride>> {
        Arc::new(|_chart_name: String| -> Option<CustomerHelmChartsOverride> {
            Some(CustomerHelmChartsOverride {
                chart_name: PrometheusAdapterChart::chart_name(),
                chart_values: "".to_string(),
            })
        })
    }

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn kube_prometheus_stack_chart_directory_exists_test() {
        // setup:
        let chart = PrometheusAdapterChart::new(
            HelmAction::Deploy,
            None,
            "whatever".to_string(),
            HelmChartNamespaces::Prometheus,
            get_prometheus_adapter_chart_override(),
            false,
            false,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared),
            PrometheusAdapterChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn kube_prometheus_stack_chart_values_file_exists_test() {
        // setup:
        let chart = PrometheusAdapterChart::new(
            HelmAction::Deploy,
            None,
            "whatever".to_string(),
            HelmChartNamespaces::Prometheus,
            get_prometheus_adapter_chart_override(),
            false,
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
            PrometheusAdapterChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
    }

    /// Make sure rust code doesn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn kube_prometheus_stack_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = PrometheusAdapterChart::new(
            HelmAction::Deploy,
            None,
            "whatever".to_string(),
            HelmChartNamespaces::Prometheus,
            get_prometheus_adapter_chart_override(),
            false,
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
                PrometheusAdapterChart::chart_name()
            ),
        );

        // verify:
        assert!(
            missing_fields.is_none(),
            "Some fields are missing in values file, add those (make sure they still exist in chart values), fields: {}",
            missing_fields.unwrap_or_default().join(",")
        );
    }
}

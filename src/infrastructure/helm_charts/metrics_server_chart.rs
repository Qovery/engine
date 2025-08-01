use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, CommonChartVpa, HelmChartError,
    HelmChartNamespaces, UpdateStrategy, VpaConfig, VpaContainerPolicy, VpaTargetRef, VpaTargetRefApiVersion,
    VpaTargetRefKind,
};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartResources, HelmChartResourcesConstraintType,
    HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::io_models::models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};
use kube::Client;

pub struct MetricsServerChart {
    chart_prefix_path: Option<String>,
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    chart_resources: HelmChartResources,
    namespace: HelmChartNamespaces,
    update_strategy: UpdateStrategy,
    enable_vpa: bool,
    allow_insecure_tls: bool,
}

impl MetricsServerChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        chart_resources: HelmChartResourcesConstraintType,
        namespace: HelmChartNamespaces,
        update_strategy: UpdateStrategy,
        enable_vpa: bool,
        allow_insecure_tls: bool,
    ) -> MetricsServerChart {
        MetricsServerChart {
            chart_prefix_path: chart_prefix_path.map(|s| s.to_string()),
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                MetricsServerChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                MetricsServerChart::chart_name(),
            ),
            chart_resources: match chart_resources {
                HelmChartResourcesConstraintType::Constrained(r) => r,
                HelmChartResourcesConstraintType::ChartDefault => HelmChartResources {
                    limit_cpu: KubernetesCpuResourceUnit::MilliCpu(250),
                    limit_memory: KubernetesMemoryResourceUnit::MebiByte(256),
                    request_cpu: KubernetesCpuResourceUnit::MilliCpu(250),
                    request_memory: KubernetesMemoryResourceUnit::MebiByte(256),
                },
            },
            namespace,
            update_strategy,
            enable_vpa,
            allow_insecure_tls,
        }
    }

    pub fn chart_name() -> String {
        "metrics-server".to_string()
    }
}

impl ToCommonHelmChart for MetricsServerChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        let mut common_chart = CommonChart {
            chart_info: ChartInfo {
                name: MetricsServerChart::chart_name(),
                path: self.chart_path.to_string(),
                namespace: self.namespace.clone(),
                values_files: vec![self.chart_values_path.to_string()],
                values: vec![
                    ChartSetValue {
                        key: "updateStrategy.type".to_string(),
                        value: self.update_strategy.to_string(),
                    },
                    ChartSetValue {
                        key: "resources.limits.cpu".to_string(),
                        value: self.chart_resources.limit_cpu.to_string(),
                    },
                    ChartSetValue {
                        key: "resources.limits.memory".to_string(),
                        value: self.chart_resources.limit_memory.to_string(),
                    },
                    ChartSetValue {
                        key: "resources.requests.cpu".to_string(),
                        value: self.chart_resources.request_cpu.to_string(),
                    },
                    ChartSetValue {
                        key: "resources.requests.memory".to_string(),
                        value: self.chart_resources.request_memory.to_string(),
                    },
                ],
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(MetricsServerChartChecker::new())),
            vertical_pod_autoscaler: match self.enable_vpa {
                true => Some(CommonChartVpa::new(
                    self.chart_prefix_path.clone().unwrap_or(".".to_string()),
                    vec![VpaConfig {
                        target_ref: VpaTargetRef::new(
                            VpaTargetRefApiVersion::AppsV1,
                            VpaTargetRefKind::Deployment,
                            "metrics-server".to_string(),
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
        };

        if self.allow_insecure_tls {
            common_chart.chart_info.values.push(ChartSetValue {
                key: "args[0]".to_string(),
                value: "--kubelet-insecure-tls".to_string(),
            });
        }

        Ok(common_chart)
    }
}

#[derive(Clone)]
pub struct MetricsServerChartChecker {}

impl MetricsServerChartChecker {
    pub fn new() -> MetricsServerChartChecker {
        MetricsServerChartChecker {}
    }
}

impl Default for MetricsServerChartChecker {
    fn default() -> Self {
        MetricsServerChartChecker::new()
    }
}

impl ChartInstallationChecker for MetricsServerChartChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO(ENG-1393): Implement chart install verification
        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::helm::{HelmChartNamespaces, UpdateStrategy};
    use crate::infrastructure::helm_charts::metrics_server_chart::MetricsServerChart;
    use crate::infrastructure::helm_charts::{
        HelmChartResourcesConstraintType, HelmChartType, ToCommonHelmChart,
        get_helm_path_kubernetes_provider_sub_folder_name, get_helm_values_set_in_code_but_absent_in_values_file,
    };
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn metrics_server_chart_directory_exists_test() {
        // setup:
        let chart = MetricsServerChart::new(
            None,
            HelmChartResourcesConstraintType::ChartDefault,
            HelmChartNamespaces::Qovery,
            UpdateStrategy::Recreate,
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
            MetricsServerChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn metrics_server_chart_values_file_exists_test() {
        // setup:
        let chart = MetricsServerChart::new(
            None,
            HelmChartResourcesConstraintType::ChartDefault,
            HelmChartNamespaces::Qovery,
            UpdateStrategy::Recreate,
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
            MetricsServerChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
    }

    /// Make sure rust code doesn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn metrics_server_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = MetricsServerChart::new(
            None,
            HelmChartResourcesConstraintType::ChartDefault,
            HelmChartNamespaces::Qovery,
            UpdateStrategy::Recreate,
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
                MetricsServerChart::chart_name(),
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

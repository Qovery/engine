use crate::cloud_provider::helm::{ChartInfo, ChartInstallationChecker, CommonChart, HelmChartNamespaces};
use crate::cloud_provider::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::errors::CommandError;
use kube::Client;
use semver::Version;

pub struct KubeStateMetricsChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
}

impl KubeStateMetricsChart {
    pub fn new(chart_prefix_path: Option<&str>) -> KubeStateMetricsChart {
        KubeStateMetricsChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                KubeStateMetricsChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                KubeStateMetricsChart::chart_name(),
            ),
        }
    }

    pub fn chart_name() -> String {
        "kube-state-metrics".to_string()
    }
}

impl ToCommonHelmChart for KubeStateMetricsChart {
    fn to_common_helm_chart(&self) -> CommonChart {
        CommonChart {
            chart_info: ChartInfo {
                name: KubeStateMetricsChart::chart_name(),
                namespace: HelmChartNamespaces::Prometheus,
                last_breaking_version_requiring_restart: Some(Version::new(4, 23, 0)),
                path: self.chart_path.to_string(),
                values_files: vec![self.chart_values_path.to_string()],
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(KubeStateMetricsChartChecker::new())),
        }
    }
}

pub struct KubeStateMetricsChartChecker {}

impl KubeStateMetricsChartChecker {
    pub fn new() -> KubeStateMetricsChartChecker {
        KubeStateMetricsChartChecker {}
    }
}

impl Default for KubeStateMetricsChartChecker {
    fn default() -> Self {
        KubeStateMetricsChartChecker::new()
    }
}

impl ChartInstallationChecker for KubeStateMetricsChartChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO(ENG-1394): Implement chart install verification
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::helm_charts::kube_state_metrics::KubeStateMetricsChart;
    use crate::cloud_provider::helm_charts::{
        get_helm_path_kubernetes_provider_sub_folder_name, get_helm_values_set_in_code_but_absent_in_values_file,
        HelmChartType, ToCommonHelmChart,
    };
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn kube_state_metrics_chart_directory_exists_test() {
        // setup:
        let chart = KubeStateMetricsChart::new(None);

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared,),
            KubeStateMetricsChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{}`", chart_path);
    }

    /// Makes sure chart values file exists.
    #[test]
    fn kube_state_metrics_chart_values_file_exists_test() {
        // setup:
        let chart = KubeStateMetricsChart::new(None);

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_values_path = format!(
            "{}/lib/{}/bootstrap/chart_values/{}.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart.chart_values_path.helm_path(),
                HelmChartType::Shared,
            ),
            KubeStateMetricsChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{}`", chart_values_path);
    }

    /// Make sure rust code deosn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn kube_state_metrics_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = KubeStateMetricsChart::new(None);
        let common_chart = chart.to_common_helm_chart();

        // execute:
        let missing_fields = get_helm_values_set_in_code_but_absent_in_values_file(
            common_chart,
            format!(
                "/lib/{}/bootstrap/chart_values/{}.yaml",
                get_helm_path_kubernetes_provider_sub_folder_name(
                    chart.chart_values_path.helm_path(),
                    HelmChartType::Shared,
                ),
                KubeStateMetricsChart::chart_name()
            ),
        );

        // verify:
        assert!(missing_fields.is_none(), "Some fields are missing in values file, add those (make sure they still exist in chart values), fields: {}", missing_fields.unwrap_or_default().join(","));
    }
}

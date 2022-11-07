use crate::cloud_provider::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmChartNamespaces,
};
use crate::cloud_provider::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::errors::CommandError;
use kube::Client;
use semver::Version;

pub struct PrometheusAdapterChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    prometheus_internal_url: String,
    prometheus_namespace: HelmChartNamespaces,
}

impl PrometheusAdapterChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        prometheus_url: String,
        prometheus_namespace: HelmChartNamespaces,
    ) -> PrometheusAdapterChart {
        PrometheusAdapterChart {
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
        }
    }

    pub fn chart_name() -> String {
        "prometheus-adapter".to_string()
    }
}

impl ToCommonHelmChart for PrometheusAdapterChart {
    fn to_common_helm_chart(&self) -> CommonChart {
        CommonChart {
            chart_info: ChartInfo {
                name: "prometheus-adapter".to_string(),
                path: self.chart_path.to_string(),
                last_breaking_version_requiring_restart: Some(Version::new(3, 3, 1)),
                namespace: self.prometheus_namespace,
                values_files: vec![self.chart_values_path.to_string()],
                values: vec![ChartSetValue {
                    key: "prometheus.url".to_string(),
                    value: self.prometheus_internal_url.clone(),
                }],
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

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
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::helm::HelmChartNamespaces;
    use crate::cloud_provider::helm_charts::prometheus_adapter_chart::PrometheusAdapterChart;
    use crate::cloud_provider::helm_charts::{
        get_helm_values_set_in_code_but_absent_in_values_file, ToCommonHelmChart,
    };
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn kube_prometheus_stack_chart_directory_exists_test() {
        // setup:
        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/common/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            PrometheusAdapterChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{}`", chart_path);
    }

    /// Makes sure chart values file exists.
    #[test]
    fn kube_prometheus_stack_chart_values_file_exists_test() {
        // setup:
        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_values_path = format!(
            "{}/lib/common/bootstrap/chart_values/{}.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            PrometheusAdapterChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{}`", chart_values_path);
    }

    /// Make sure rust code doesn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = PrometheusAdapterChart::new(None, "whatever".to_string(), HelmChartNamespaces::Prometheus)
            .to_common_helm_chart();

        // execute:
        let missing_fields = get_helm_values_set_in_code_but_absent_in_values_file(
            chart,
            format!(
                "/lib/common/bootstrap/chart_values/{}.yaml",
                PrometheusAdapterChart::chart_name()
            ),
        );

        // verify:
        assert!(missing_fields.is_none(), "Some fields are missing in values file, add those (make sure they still exist in chart values), fields: {}", missing_fields.unwrap_or_default().join(","));
    }
}

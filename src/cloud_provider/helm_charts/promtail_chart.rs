use crate::cloud_provider::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmChartNamespaces,
};
use crate::cloud_provider::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::errors::CommandError;
use kube::Client;
use semver::Version;

pub struct PromtailChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    loki_kube_dns_name: String,
}

impl PromtailChart {
    pub fn new(chart_prefix_path: Option<&str>, loki_kube_dns_name: String) -> Self {
        PromtailChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                PromtailChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                PromtailChart::chart_name(),
            ),
            loki_kube_dns_name,
        }
    }

    fn chart_name() -> String {
        "promtail".to_string()
    }
}

impl ToCommonHelmChart for PromtailChart {
    fn to_common_helm_chart(&self) -> CommonChart {
        CommonChart {
            chart_info: ChartInfo {
                name: PromtailChart::chart_name(),
                reinstall_chart_if_installed_version_is_below_than: Some(Version::new(5, 1, 0)),
                path: self.chart_path.to_string(),
                // because of priorityClassName, we need to add it to kube-system
                namespace: HelmChartNamespaces::KubeSystem,
                values_files: vec![self.chart_values_path.to_string()],
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
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(PromtailChartChecker::new())),
        }
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
    use crate::cloud_provider::helm_charts::promtail_chart::PromtailChart;
    use crate::cloud_provider::helm_charts::{
        get_helm_path_kubernetes_provider_sub_folder_name, get_helm_values_set_in_code_but_absent_in_values_file,
        HelmChartType, ToCommonHelmChart,
    };
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn promtail_chart_directory_exists_test() {
        // setup:
        let chart = PromtailChart::new(None, "whatever".to_string());

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
        let chart = PromtailChart::new(None, "whatever".to_string());

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
        let chart = PromtailChart::new(None, "whatever".to_string());
        let common_chart = chart.to_common_helm_chart();

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

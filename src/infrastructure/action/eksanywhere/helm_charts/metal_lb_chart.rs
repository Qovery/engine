use crate::helm::{ChartInfo, CommonChart, HelmChartError, HelmChartNamespaces};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};

pub struct MetalLbChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    namespace: HelmChartNamespaces,
}

impl MetalLbChart {
    pub fn new(chart_prefix_path: Option<&str>, namespace: HelmChartNamespaces) -> MetalLbChart {
        MetalLbChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                MetalLbChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                MetalLbChart::chart_name(),
            ),
            namespace,
        }
    }
    fn chart_name() -> String {
        "metallb".to_string()
    }
}

impl ToCommonHelmChart for MetalLbChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        Ok(CommonChart {
            chart_info: ChartInfo {
                name: MetalLbChart::chart_name(),
                namespace: self.namespace.clone(),
                path: self.chart_path.to_string(),
                values: vec![],
                values_files: vec![self.chart_values_path.to_string()],
                ..Default::default()
            },
            vertical_pod_autoscaler: None,
            chart_installation_checker: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::helm::HelmChartNamespaces;
    use crate::infrastructure::action::eksanywhere::helm_charts::metal_lb_chart::MetalLbChart;
    use crate::infrastructure::helm_charts::{
        HelmChartType, ToCommonHelmChart, get_helm_path_kubernetes_provider_sub_folder_name,
        get_helm_values_set_in_code_but_absent_in_values_file,
    };
    use crate::infrastructure::models::kubernetes::Kind::EksAnywhere;
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn metal_lb_chart_directory_exits_test() {
        let chart = MetalLbChart::new(None, HelmChartNamespaces::Qovery);

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart.chart_path.helm_path(),
                HelmChartType::CloudProviderSpecific(EksAnywhere)
            ),
            MetalLbChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn metal_lb_chart_values_file_exists_test() {
        // setup:
        let chart = MetalLbChart::new(None, HelmChartNamespaces::Qovery);

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_values_path = format!(
            "{}/lib/{}/bootstrap/chart_values/{}.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart.chart_values_path.helm_path(),
                HelmChartType::CloudProviderSpecific(EksAnywhere),
            ),
            MetalLbChart::chart_name(),
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
        let chart = MetalLbChart::new(None, HelmChartNamespaces::Qovery);
        let common_chart = chart.to_common_helm_chart().unwrap();

        // execute:
        let missing_fields = get_helm_values_set_in_code_but_absent_in_values_file(
            common_chart,
            format!(
                "/lib/{}/bootstrap/chart_values/{}.yaml",
                get_helm_path_kubernetes_provider_sub_folder_name(
                    chart.chart_values_path.helm_path(),
                    HelmChartType::CloudProviderSpecific(EksAnywhere),
                ),
                MetalLbChart::chart_name(),
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

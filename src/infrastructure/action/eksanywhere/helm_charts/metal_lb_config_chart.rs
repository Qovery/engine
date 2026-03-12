use crate::helm::{ChartInfo, ChartSetValue, CommonChart, HelmChartError, HelmChartNamespaces};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};

pub struct MetalLbConfigChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    namespace: HelmChartNamespaces,
    ip_address_pools: Vec<String>,
}

impl MetalLbConfigChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        namespace: HelmChartNamespaces,
        ip_address_pool: Vec<String>,
    ) -> MetalLbConfigChart {
        MetalLbConfigChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                MetalLbConfigChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                MetalLbConfigChart::chart_name(),
            ),
            namespace,
            ip_address_pools: ip_address_pool,
        }
    }
    fn chart_name() -> String {
        "metallb-config".to_string()
    }
}

impl ToCommonHelmChart for MetalLbConfigChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        let mut values = vec![];
        self.ip_address_pools
            .iter()
            .enumerate()
            .for_each(|(index, address_pool)| {
                let prefix = format!("ipAddressPool.addresses[{index}]");
                values.push(ChartSetValue {
                    key: prefix.to_string(),
                    value: address_pool.to_string(),
                });
            });
        Ok(CommonChart {
            chart_info: ChartInfo {
                name: MetalLbConfigChart::chart_name(),
                namespace: self.namespace.clone(),
                path: self.chart_path.to_string(),
                values,
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
    use crate::infrastructure::action::eksanywhere::helm_charts::metal_lb_config_chart::MetalLbConfigChart;
    use crate::infrastructure::helm_charts::{
        HelmChartType, ToCommonHelmChart, get_helm_path_kubernetes_provider_sub_folder_name,
        get_helm_values_set_in_code_but_absent_in_values_file,
    };
    use crate::infrastructure::models::kubernetes::Kind::EksAnywhere;
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn metal_lb_chart_directory_exits_test() {
        let chart =
            MetalLbConfigChart::new(None, HelmChartNamespaces::Qovery, vec!["192.168.0.1-192.168.0.10".to_string()]);

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
            MetalLbConfigChart::chart_name(),
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
        let chart =
            MetalLbConfigChart::new(None, HelmChartNamespaces::Qovery, vec!["192.168.0.1-192.168.0.10".to_string()]);

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
            MetalLbConfigChart::chart_name(),
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
        let chart =
            MetalLbConfigChart::new(None, HelmChartNamespaces::Qovery, vec!["192.168.0.1-192.168.0.10".to_string()]);
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
                MetalLbConfigChart::chart_name(),
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

use crate::cloud_provider::helm::{ChartInfo, ChartSetValue, CommonChart, HelmChartError, HelmChartNamespaces};
use crate::cloud_provider::helm_charts::{HelmChartDirectoryLocation, HelmChartPath, ToCommonHelmChart};

pub struct QoveryPdbInfraChart {
    chart_path: HelmChartPath,
    namespace: HelmChartNamespaces,
    prometheus_namespace: HelmChartNamespaces,
    loki_namespace: HelmChartNamespaces,
}

impl QoveryPdbInfraChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        namespace: HelmChartNamespaces,
        prometheus_namespace: HelmChartNamespaces,
        loki_namespace: HelmChartNamespaces,
    ) -> Self {
        QoveryPdbInfraChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                QoveryPdbInfraChart::chart_name(),
            ),
            namespace,
            prometheus_namespace,
            loki_namespace,
        }
    }

    pub fn chart_name() -> String {
        "qovery-pdb-infra".to_string()
    }
}

impl ToCommonHelmChart for QoveryPdbInfraChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        Ok(CommonChart {
            chart_info: ChartInfo {
                name: QoveryPdbInfraChart::chart_name(),
                namespace: self.namespace,
                path: self.chart_path.to_string(),
                values_files: vec![],
                values: vec![
                    ChartSetValue {
                        key: "loki.namespace".to_string(),
                        value: self.loki_namespace.to_string(),
                    },
                    ChartSetValue {
                        key: "prometheus.namespace".to_string(),
                        value: self.prometheus_namespace.to_string(),
                    },
                ],
                ..Default::default()
            },
            chart_installation_checker: None,
            vertical_pod_autoscaler: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud_provider::helm_charts::{get_helm_path_kubernetes_provider_sub_folder_name, HelmChartType};
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn qovery_pdb_infra_chart_directory_exists_test() {
        // setup:
        let chart = QoveryPdbInfraChart::new(
            None,
            HelmChartNamespaces::Qovery,
            HelmChartNamespaces::Prometheus,
            HelmChartNamespaces::Logging,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared,),
            QoveryPdbInfraChart::chart_name()
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }
}

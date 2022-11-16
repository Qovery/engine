use crate::cloud_provider::helm::{ChartInfo, ChartInstallationChecker, CommonChart, HelmChartNamespaces};
use crate::cloud_provider::helm_charts::{HelmChartDirectoryLocation, HelmChartPath, ToCommonHelmChart};
use crate::errors::CommandError;
use kube::Client;

pub struct AwsUiViewChart {
    chart_path: HelmChartPath,
}

impl AwsUiViewChart {
    pub fn new(chart_prefix_path: Option<&str>) -> AwsUiViewChart {
        AwsUiViewChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                AwsUiViewChart::chart_name(),
            ),
        }
    }

    fn chart_name() -> String {
        "aws-ui-view".to_string()
    }
}

impl ToCommonHelmChart for AwsUiViewChart {
    fn to_common_helm_chart(&self) -> CommonChart {
        CommonChart {
            chart_info: ChartInfo {
                name: AwsUiViewChart::chart_name(),
                path: self.chart_path.to_string(),
                namespace: HelmChartNamespaces::KubeSystem,
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(AwsUiViewChartChecker::new())),
        }
    }
}

pub struct AwsUiViewChartChecker {}

impl AwsUiViewChartChecker {
    pub fn new() -> AwsUiViewChartChecker {
        AwsUiViewChartChecker {}
    }
}

impl Default for AwsUiViewChartChecker {
    fn default() -> Self {
        AwsUiViewChartChecker::new()
    }
}

impl ChartInstallationChecker for AwsUiViewChartChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO(ENG-1365): Implement chart install verification
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::aws::kubernetes::helm_charts::aws_ui_view_chart::AwsUiViewChart;
    use crate::cloud_provider::helm_charts::get_helm_path_kubernetes_provider_sub_folder_name;
    use crate::cloud_provider::kubernetes::Kind as KubernetesKind;
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn aws_vpc_cni_chart_directory_exists_test() {
        // setup:
        let chart = AwsUiViewChart::new(None);

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), Some(KubernetesKind::Eks)),
            AwsUiViewChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{}`", chart_path);
    }
}

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
            chart_installation_checker: Some(Box::new(AwsUiViewChecker::new())),
        }
    }
}

pub struct AwsUiViewChecker {}

impl AwsUiViewChecker {
    pub fn new() -> AwsUiViewChecker {
        AwsUiViewChecker {}
    }
}

impl Default for AwsUiViewChecker {
    fn default() -> Self {
        AwsUiViewChecker::new()
    }
}

impl ChartInstallationChecker for AwsUiViewChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO(ENG-1365): Implement chart install verification
        Ok(())
    }
}

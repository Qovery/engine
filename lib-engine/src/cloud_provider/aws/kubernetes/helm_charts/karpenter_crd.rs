use crate::cloud_provider::helm::{
    ChartInfo, ChartInstallationChecker, CommonChart, HelmChartError, HelmChartNamespaces,
};
use crate::cloud_provider::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::errors::CommandError;
use kube::Client;

pub struct KarpenterCrdChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
}

impl KarpenterCrdChart {
    pub fn new(chart_prefix_path: Option<&str>) -> Self {
        KarpenterCrdChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                KarpenterCrdChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                KarpenterCrdChart::chart_name(),
            ),
        }
    }

    pub fn chart_name() -> String {
        "karpenter-crd".to_string()
    }
}

impl ToCommonHelmChart for KarpenterCrdChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        Ok(CommonChart {
            chart_info: ChartInfo {
                name: KarpenterCrdChart::chart_name(),
                namespace: HelmChartNamespaces::KubeSystem,
                path: self.chart_path.to_string(),
                values_files: vec![self.chart_values_path.to_string()],
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(KarpenterCrdChartChecker::new())),
            vertical_pod_autoscaler: None, // enabled in the chart configuration
        })
    }
}

#[derive(Clone)]
pub struct KarpenterCrdChartChecker {}

impl KarpenterCrdChartChecker {
    pub fn new() -> KarpenterCrdChartChecker {
        KarpenterCrdChartChecker {}
    }
}

impl Default for KarpenterCrdChartChecker {
    fn default() -> Self {
        KarpenterCrdChartChecker::new()
    }
}

impl ChartInstallationChecker for KarpenterCrdChartChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO(ENG-1366): Implement chart install verification
        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

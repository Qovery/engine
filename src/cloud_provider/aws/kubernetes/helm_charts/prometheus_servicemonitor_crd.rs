use crate::cloud_provider::helm::{
    ChartInfo, ChartInstallationChecker, CommonChart, HelmAction, HelmChartError, HelmChartNamespaces,
};
use crate::cloud_provider::helm_charts::{HelmChartDirectoryLocation, HelmChartPath, ToCommonHelmChart};
use crate::errors::CommandError;
use kube::Client;

#[derive(Clone)]
pub struct PrometheusServiceMonitorCrdChart {
    chart_path: HelmChartPath,
}

impl PrometheusServiceMonitorCrdChart {
    pub fn new(chart_prefix_path: Option<&str>) -> Self {
        PrometheusServiceMonitorCrdChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                PrometheusServiceMonitorCrdChart::chart_name(),
            ),
        }
    }

    pub fn chart_name() -> String {
        "prometheus-servicemonitor-crd".to_string()
    }
}

impl ToCommonHelmChart for PrometheusServiceMonitorCrdChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        Ok(CommonChart {
            chart_info: ChartInfo {
                name: PrometheusServiceMonitorCrdChart::chart_name(),
                action: HelmAction::Deploy,
                namespace: HelmChartNamespaces::KubeSystem,
                path: self.chart_path.to_string(),
                values: vec![],
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(PrometheusServiceMonitorCrdChartChecker::new())),
            vertical_pod_autoscaler: None, // enabled in the chart configuration
        })
    }
}

#[derive(Clone)]
pub struct PrometheusServiceMonitorCrdChartChecker {}

impl PrometheusServiceMonitorCrdChartChecker {
    pub fn new() -> PrometheusServiceMonitorCrdChartChecker {
        PrometheusServiceMonitorCrdChartChecker {}
    }
}

impl Default for PrometheusServiceMonitorCrdChartChecker {
    fn default() -> Self {
        PrometheusServiceMonitorCrdChartChecker::new()
    }
}

impl ChartInstallationChecker for PrometheusServiceMonitorCrdChartChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO(ENG-1366): Implement chart install verification
        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

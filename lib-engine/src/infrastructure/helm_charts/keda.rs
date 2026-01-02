use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmAction, HelmChartError, HelmChartNamespaces,
};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use kube::Client;

pub struct KedaChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    enable_monitoring: bool,
    action: HelmAction,
    keda_operator_role_arn: Option<String>,
    keda_metrics_server_role_arn: Option<String>,
}

impl KedaChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        enable_monitoring: bool,
        action: HelmAction,
        keda_operator_role_arn: Option<String>,
        keda_metrics_server_role_arn: Option<String>,
    ) -> Self {
        KedaChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                KedaChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                KedaChart::chart_name(),
            ),
            enable_monitoring,
            action,
            keda_operator_role_arn,
            keda_metrics_server_role_arn,
        }
    }

    pub fn chart_name() -> String {
        "keda".to_string()
    }
}

impl ToCommonHelmChart for KedaChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        let mut values = vec![
            ChartSetValue {
                key: "prometheus.operator.enabled".to_string(),
                value: self.enable_monitoring.to_string(),
            },
            ChartSetValue {
                key: "prometheus.operator.podMonitor.enabled".to_string(),
                value: self.enable_monitoring.to_string(),
            },
            ChartSetValue {
                key: "prometheus.operator.prometheusRule.enabled".to_string(),
                value: self.enable_monitoring.to_string(),
            },
        ];

        // Add KEDA operator role ARN if provided
        if let Some(operator_role_arn) = &self.keda_operator_role_arn {
            values.push(ChartSetValue {
                key: r"serviceAccount.annotations.eks\.amazonaws\.com/role-arn".to_string(),
                value: operator_role_arn.clone(),
            });
        }

        // Add KEDA metrics server role ARN if provided
        if let Some(metrics_server_role_arn) = &self.keda_metrics_server_role_arn {
            values.push(ChartSetValue {
                key: r"metricsServer.serviceAccount.annotations.eks\.amazonaws\.com/role-arn".to_string(),
                value: metrics_server_role_arn.clone(),
            });
        }

        Ok(CommonChart {
            chart_info: ChartInfo {
                name: KedaChart::chart_name(),
                action: self.action.clone(),
                namespace: HelmChartNamespaces::KubeSystem,
                path: self.chart_path.to_string(),
                values_files: vec![self.chart_values_path.to_string()],
                values,
                ..Default::default()
            },
            chart_installation_checker: match self.action {
                HelmAction::Deploy => Some(Box::new(KedaChartChecker::new())),
                HelmAction::Destroy => None,
            },
            vertical_pod_autoscaler: None,
        })
    }
}

#[derive(Clone)]
pub struct KedaChartChecker {}

impl KedaChartChecker {
    pub fn new() -> Self {
        KedaChartChecker {}
    }
}

impl Default for KedaChartChecker {
    fn default() -> Self {
        KedaChartChecker::new()
    }
}

impl ChartInstallationChecker for KedaChartChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO: Implement verification
        // Could check for keda-operator deployment in kube-system namespace
        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

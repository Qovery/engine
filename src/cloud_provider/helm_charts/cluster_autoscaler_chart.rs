use crate::cloud_provider::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmChartNamespaces,
};
use crate::cloud_provider::helm_charts::{HelmChartDirectoryLocation, HelmChartPath, ToCommonHelmChart};
use crate::errors::CommandError;
use kube::Client;

pub struct ClusterAutoscalerChart {
    chart_path: HelmChartPath,
    cloud_provider: String, // TODO(benjaminch): Pass cloud provider type here instead of string
    chart_image_region: String,
    cluster_name: String,
    aws_iam_cluster_autoscaler_key: String,
    aws_iam_cluster_autoscaler_secret: String,
    prometheus_namespace: HelmChartNamespaces,
    ff_metrics_history_enabled: bool,
}

impl ClusterAutoscalerChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        cloud_provider: String,
        chart_image_region: String,
        cluster_name: String,
        aws_iam_cluster_autoscaler_key: String,
        aws_iam_cluster_autoscaler_secret: String,
        prometheus_namespace: HelmChartNamespaces,
        ff_metrics_history_enabled: bool,
    ) -> Self {
        ClusterAutoscalerChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                ClusterAutoscalerChart::chart_name(),
            ),
            cloud_provider,
            chart_image_region,
            cluster_name,
            aws_iam_cluster_autoscaler_key,
            aws_iam_cluster_autoscaler_secret,
            prometheus_namespace,
            ff_metrics_history_enabled,
        }
    }

    fn chart_name() -> String {
        "cluster-autoscaler".to_string()
    }
}

impl ToCommonHelmChart for ClusterAutoscalerChart {
    fn to_common_helm_chart(&self) -> CommonChart {
        CommonChart {
            chart_info: ChartInfo {
                name: ClusterAutoscalerChart::chart_name(),
                path: self.chart_path.to_string(),
                values: vec![
                    ChartSetValue {
                        key: "cloudProvider".to_string(),
                        value: self.cloud_provider.to_string(),
                    },
                    ChartSetValue {
                        key: "awsRegion".to_string(),
                        value: self.chart_image_region.to_string(),
                    },
                    ChartSetValue {
                        key: "autoDiscovery.clusterName".to_string(),
                        value: self.cluster_name.to_string(),
                    },
                    ChartSetValue {
                        key: "awsAccessKeyID".to_string(),
                        value: self.aws_iam_cluster_autoscaler_key.to_string(),
                    },
                    ChartSetValue {
                        key: "awsSecretAccessKey".to_string(),
                        value: self.aws_iam_cluster_autoscaler_secret.to_string(),
                    },
                    // It's mandatory to get this class to ensure paused infra will behave properly on restore
                    ChartSetValue {
                        key: "priorityClassName".to_string(),
                        value: "system-cluster-critical".to_string(),
                    },
                    // cluster autoscaler options
                    ChartSetValue {
                        key: "extraArgs.balance-similar-node-groups".to_string(),
                        value: "true".to_string(),
                    },
                    // observability
                    ChartSetValue {
                        key: "serviceMonitor.enabled".to_string(),
                        value: self.ff_metrics_history_enabled.to_string(),
                    },
                    ChartSetValue {
                        key: "serviceMonitor.namespace".to_string(),
                        value: self.prometheus_namespace.to_string(),
                    },
                    // resources limits
                    ChartSetValue {
                        key: "resources.limits.cpu".to_string(),
                        value: "100m".to_string(),
                    },
                    ChartSetValue {
                        key: "resources.requests.cpu".to_string(),
                        value: "100m".to_string(),
                    },
                    ChartSetValue {
                        key: "resources.limits.memory".to_string(),
                        value: "640Mi".to_string(),
                    },
                    ChartSetValue {
                        key: "resources.requests.memory".to_string(),
                        value: "640Mi".to_string(),
                    },
                ],
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(ClusterAutoscalerChartChecker::new())),
        }
    }
}

pub struct ClusterAutoscalerChartChecker {}

impl ClusterAutoscalerChartChecker {
    pub fn new() -> ClusterAutoscalerChartChecker {
        ClusterAutoscalerChartChecker {}
    }
}

impl Default for ClusterAutoscalerChartChecker {
    fn default() -> Self {
        ClusterAutoscalerChartChecker::new()
    }
}

impl ChartInstallationChecker for ClusterAutoscalerChartChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO(ENG-1366): Implement chart install verification
        Ok(())
    }
}

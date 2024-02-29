use crate::cloud_provider::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmAction, HelmChartError, HelmChartNamespaces,
};
use crate::cloud_provider::helm_charts::{HelmChartDirectoryLocation, HelmChartPath, ToCommonHelmChart};
use crate::errors::CommandError;
use kube::Client;

pub struct KarpenterChart {
    chart_path: HelmChartPath,
    cluster_name: String,
    aws_iam_karpenter_controller_role_arn: String,
    replace_cluster_autoscaler: bool,
}

impl KarpenterChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        cluster_name: String,
        aws_iam_karpenter_controller_role_arn: String,
        replace_cluster_autoscaler: bool,
    ) -> Self {
        KarpenterChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                KarpenterChart::chart_name(),
            ),
            cluster_name,
            aws_iam_karpenter_controller_role_arn,
            replace_cluster_autoscaler,
        }
    }

    fn chart_name() -> String {
        "karpenter".to_string()
    }
}

impl ToCommonHelmChart for KarpenterChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        Ok(CommonChart {
            chart_info: ChartInfo {
                name: KarpenterChart::chart_name(),
                action: match self.replace_cluster_autoscaler {
                    true => HelmAction::Deploy,
                    false => HelmAction::Destroy,
                },
                namespace: HelmChartNamespaces::KubeSystem,
                path: self.chart_path.to_string(),
                values: vec![
                    ChartSetValue {
                        key: "logLevel".to_string(),
                        value: "debug".to_string(),
                    },
                    // ChartSetValue {
                    //     key: "settings.aws.defaultInstanceProfile".to_string(),
                    //     value: format!("KarpenterNodeInstanceProfile-{}", self.cluster_name.clone()),
                    // },
                    ChartSetValue {
                        key: "settings.clusterName".to_string(),
                        value: self.cluster_name.to_string(),
                    },
                    ChartSetValue {
                        key: r"serviceAccount.annotations.eks\.amazonaws\.com/role-arn".to_string(),
                        value: self.aws_iam_karpenter_controller_role_arn.to_string(),
                    },
                ],
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(KarpenterChartChecker::new())),
            vertical_pod_autoscaler: None, // enabled in the chart configuration
        })
    }
}

#[derive(Clone)]
pub struct KarpenterChartChecker {}

impl KarpenterChartChecker {
    pub fn new() -> KarpenterChartChecker {
        KarpenterChartChecker {}
    }
}

impl Default for KarpenterChartChecker {
    fn default() -> Self {
        KarpenterChartChecker::new()
    }
}

impl ChartInstallationChecker for KarpenterChartChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO(ENG-1366): Implement chart install verification
        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

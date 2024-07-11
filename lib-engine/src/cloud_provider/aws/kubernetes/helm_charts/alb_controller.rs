use crate::cloud_provider::{
    helm::{ChartInfo, ChartInstallationChecker, ChartSetValue, HelmChart, HelmChartNamespaces},
    helm_charts::{HelmChartDirectoryLocation, HelmChartPath},
};

#[derive(Clone)]
pub struct AwsLoadBalancerControllerChart {
    pub chart_info: ChartInfo,
}

impl AwsLoadBalancerControllerChart {
    pub fn new(chart_prefix_path: Option<&str>, aws_alb_controller_role_arn: String, cluster_name: String) -> Self {
        let chart_path = HelmChartPath::new(
            chart_prefix_path,
            HelmChartDirectoryLocation::CloudProviderFolder,
            Self::chart_name(),
        );
        Self {
            chart_info: ChartInfo {
                name: "aws-load-balancer-controller".to_string(),
                namespace: HelmChartNamespaces::KubeSystem,
                path: chart_path.to_string(),
                values: vec![
                    ChartSetValue {
                        key: "replicaCount".to_string(),
                        value: "1".to_string(),
                    },
                    ChartSetValue {
                        key: "clusterName".to_string(),
                        value: cluster_name,
                    },
                    ChartSetValue {
                        key: r"serviceAccount.annotations.eks\.amazonaws\.com/role-arn".to_string(),
                        value: aws_alb_controller_role_arn,
                    },
                    ChartSetValue {
                        key: "autoscaling.enabled".to_string(),
                        value: "true".to_string(),
                    },
                ],
                ..Default::default()
            },
        }
    }

    pub fn chart_name() -> String {
        "aws-load-balancer-controller".to_string()
    }
}

#[derive(Clone)]
struct AwsLoadBalancerControllerChartChecker {}

impl AwsLoadBalancerControllerChartChecker {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {}
    }
}

impl ChartInstallationChecker for AwsLoadBalancerControllerChartChecker {
    fn verify_installation(&self, _kube_client: &kube::Client) -> Result<(), crate::errors::CommandError> {
        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

impl HelmChart for AwsLoadBalancerControllerChart {
    fn clone_dyn(&self) -> Box<dyn HelmChart> {
        Box::new(self.clone())
    }

    fn get_chart_info(&self) -> &ChartInfo {
        &self.chart_info
    }
}

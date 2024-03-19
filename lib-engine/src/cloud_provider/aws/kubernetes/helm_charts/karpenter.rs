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
    enable_monitoring: bool,
}

impl KarpenterChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        cluster_name: String,
        aws_iam_karpenter_controller_role_arn: String,
        replace_cluster_autoscaler: bool,
        _enable_monitoring: bool,
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
            enable_monitoring: false, // disable until the crd installation is fixed
        }
    }

    pub fn chart_name() -> String {
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
                    ChartSetValue {
                        key: "settings.clusterName".to_string(),
                        value: self.cluster_name.to_string(),
                    },
                    ChartSetValue {
                        key: r"serviceAccount.annotations.eks\.amazonaws\.com/role-arn".to_string(),
                        value: self.aws_iam_karpenter_controller_role_arn.to_string(),
                    },
                    ChartSetValue {
                        key: "settings.interruptionQueue".to_string(),
                        value: self.cluster_name.to_string(),
                    },
                    ChartSetValue {
                        key: "serviceMonitor.enabled".to_string(),
                        value: self.enable_monitoring.to_string(),
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

#[cfg(test)]
mod tests {
    use std::fs::File;

    #[test]
    fn test_ec2_node_classes_custom_resource_is_aligned_with_definition() {
        let filename = "./lib/aws/bootstrap/charts/karpenter/crds/karpenter.k8s.aws_ec2nodeclasses.yaml";
        let file = File::open(filename).unwrap();
        let yaml: serde_yaml::Value = serde_yaml::from_reader(file).unwrap();
        let group = &yaml["spec"]["group"];
        let version = &yaml["spec"]["versions"][0]["name"];
        let kind = &yaml["spec"]["names"]["kind"];

        // These values must be equal to the ones define in the CustomResource in the kube_client.rs file
        // #[kube(group = "karpenter.k8s.aws", version = "v1beta1", kind = "EC2NodeClass")]
        assert_eq!(group.as_str(), Some("karpenter.k8s.aws"));
        assert_eq!(version.as_str(), Some("v1beta1"));
        assert_eq!(kind.as_str(), Some("EC2NodeClass"));
    }
}

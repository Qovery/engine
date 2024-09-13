use crate::cloud_provider::{
    helm::{
        ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, CommonChartVpa, HelmChart, HelmChartError,
        HelmChartNamespaces, VpaConfig, VpaContainerPolicy, VpaTargetRef, VpaTargetRefApiVersion, VpaTargetRefKind,
    },
    helm_charts::{HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart},
    models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit},
};

#[derive(Clone)]
pub struct AwsLoadBalancerControllerChart {
    pub chart_prefix_path: Option<String>,
    pub chart_info: ChartInfo,
    pub vpa_enabled: bool,
    pub enable_mutator_webhook: bool,
}

impl AwsLoadBalancerControllerChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        aws_alb_controller_role_arn: String,
        cluster_name: String,
        vpa_enabled: bool,
        // https://kubernetes-sigs.github.io/aws-load-balancer-controller/v2.5/deploy/installation/
        enable_mutator_webhook: bool,
    ) -> Self {
        let chart_path = HelmChartPath::new(
            chart_prefix_path,
            HelmChartDirectoryLocation::CloudProviderFolder,
            Self::chart_name(),
        );
        Self {
            chart_prefix_path: chart_prefix_path.map(|s| s.to_string()),
            chart_info: ChartInfo {
                name: "aws-load-balancer-controller".to_string(),
                namespace: HelmChartNamespaces::KubeSystem,
                values_files: vec![HelmChartValuesFilePath::new(
                    chart_prefix_path,
                    HelmChartDirectoryLocation::CloudProviderFolder,
                    AwsLoadBalancerControllerChart::chart_name(),
                )
                .to_string()],
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
                        key: "enableServiceMutatorWebhook".to_string(),
                        value: enable_mutator_webhook.to_string(),
                    },
                    ChartSetValue {
                        key: "autoscaling.enabled".to_string(),
                        value: "true".to_string(),
                    },
                    ChartSetValue {
                        key: "resources.requests.cpu".to_string(),
                        value: "250m".to_string(),
                    },
                    ChartSetValue {
                        key: "resources.requests.memory".to_string(),
                        value: "128Mi".to_string(),
                    },
                    ChartSetValue {
                        key: "resources.limits.cpu".to_string(),
                        value: "250m".to_string(),
                    },
                    ChartSetValue {
                        key: "resources.limits.memory".to_string(),
                        value: "128Mi".to_string(),
                    },
                ],
                ..Default::default()
            },
            vpa_enabled,
            enable_mutator_webhook,
        }
    }

    pub fn chart_name() -> String {
        "aws-load-balancer-controller".to_string()
    }
}

impl ToCommonHelmChart for AwsLoadBalancerControllerChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        Ok(CommonChart {
            chart_info: self.chart_info.clone(),
            vertical_pod_autoscaler: match self.vpa_enabled {
                true => Some(CommonChartVpa::new(
                    self.chart_prefix_path.clone().unwrap_or(".".to_string()),
                    vec![VpaConfig::new(
                        VpaTargetRef::new(
                            VpaTargetRefApiVersion::AppsV1,
                            VpaTargetRefKind::Deployment,
                            "aws-load-balancer-controller".to_string(),
                        ),
                        VpaContainerPolicy::new(
                            "*".to_string(),
                            Some(KubernetesCpuResourceUnit::MilliCpu(128)),
                            Some(KubernetesCpuResourceUnit::MilliCpu(1000)),
                            Some(KubernetesMemoryResourceUnit::MebiByte(128)),
                            Some(KubernetesMemoryResourceUnit::GibiByte(1)),
                        ),
                    )],
                )),
                false => None,
            },
            chart_installation_checker: None,
        })
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

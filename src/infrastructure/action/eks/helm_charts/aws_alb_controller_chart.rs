use crate::helm::HelmChartError;
use crate::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, CommonChartVpa, HelmChartNamespaces, VpaConfig,
    VpaContainerPolicy, VpaTargetRef, VpaTargetRefApiVersion, VpaTargetRefKind,
};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartResources, HelmChartResourcesConstraintType,
    HelmChartValuesFilePath, HelmChartVpaType, ToCommonHelmChart,
};
use crate::io_models::models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};

pub struct AwsLoadBalancerControllerChart {
    chart_path: HelmChartPath,
    chart_prefix_path: Option<String>,
    chart_values_path: HelmChartValuesFilePath,
    chart_resources: HelmChartResources,
    chart_vpa: HelmChartVpaType,
    aws_alb_controller_role_arn: String,
    cluster_name: String,
    enable_mutator_webhook: bool,
}

impl AwsLoadBalancerControllerChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        aws_alb_controller_role_arn: String,
        cluster_name: String,
        chart_resources: HelmChartResourcesConstraintType,
        chart_vpa: HelmChartVpaType,
        // https://kubernetes-sigs.github.io/aws-load-balancer-controller/v2.5/deploy/installation/
        enable_mutator_webhook: bool,
    ) -> Self {
        Self {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                Self::chart_name(),
            ),
            chart_prefix_path: chart_prefix_path.map(|s| s.to_string()),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                Self::chart_name(),
            ),
            chart_resources: match chart_resources {
                HelmChartResourcesConstraintType::ChartDefault => HelmChartResources {
                    request_cpu: KubernetesCpuResourceUnit::MilliCpu(250),
                    request_memory: KubernetesMemoryResourceUnit::MebiByte(128),
                    limit_cpu: KubernetesCpuResourceUnit::MilliCpu(250),
                    limit_memory: KubernetesMemoryResourceUnit::MebiByte(128),
                },
                HelmChartResourcesConstraintType::Constrained(r) => r,
            },
            chart_vpa,
            aws_alb_controller_role_arn,
            cluster_name,
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
            chart_info: ChartInfo {
                name: "aws-load-balancer-controller".to_string(),
                namespace: HelmChartNamespaces::KubeSystem,
                values_files: vec![self.chart_values_path.to_string()],
                path: self.chart_path.to_string(),
                values: vec![
                    ChartSetValue {
                        key: "clusterName".to_string(),
                        value: self.cluster_name.to_string(),
                    },
                    ChartSetValue {
                        key: r"serviceAccount.annotations.eks\.amazonaws\.com/role-arn".to_string(),
                        value: self.aws_alb_controller_role_arn.to_string(),
                    },
                    ChartSetValue {
                        key: "enableServiceMutatorWebhook".to_string(),
                        value: self.enable_mutator_webhook.to_string(),
                    },
                    // resources limits
                    ChartSetValue {
                        key: "resources.limits.cpu".to_string(),
                        value: self.chart_resources.limit_cpu.to_string(),
                    },
                    ChartSetValue {
                        key: "resources.limits.memory".to_string(),
                        value: self.chart_resources.limit_memory.to_string(),
                    },
                    ChartSetValue {
                        key: "resources.requests.cpu".to_string(),
                        value: self.chart_resources.request_cpu.to_string(),
                    },
                    ChartSetValue {
                        key: "resources.requests.memory".to_string(),
                        value: self.chart_resources.request_memory.to_string(),
                    },
                ],
                ..Default::default()
            },
            vertical_pod_autoscaler: match &self.chart_vpa {
                HelmChartVpaType::Disabled => None,
                HelmChartVpaType::EnabledWithChartDefault => Some(CommonChartVpa::new(
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
                            Some(KubernetesMemoryResourceUnit::GibiByte(2)),
                        ),
                    )],
                )),
                HelmChartVpaType::EnabledWithConstraints(custom_vpa_config) => Some(CommonChartVpa::new(
                    self.chart_prefix_path.clone().unwrap_or(".".to_string()),
                    vec![VpaConfig::new(
                        VpaTargetRef::new(
                            VpaTargetRefApiVersion::AppsV1,
                            VpaTargetRefKind::Deployment,
                            "aws-load-balancer-controller".to_string(),
                        ),
                        custom_vpa_config.clone(),
                    )],
                )),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::helm_charts::{
        HelmChartType, get_helm_path_kubernetes_provider_sub_folder_name,
        get_helm_values_set_in_code_but_absent_in_values_file,
    };
    use crate::infrastructure::models::kubernetes::Kind as KubernetesKind;
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn aws_alb_controller_chart_directory_exists_test() {
        // setup:
        let chart = AwsLoadBalancerControllerChart::new(
            None,
            "arn:aws:iam::123456789012:role/eks-alb-ingress-controller".to_string(),
            "cluster-name".to_string(),
            HelmChartResourcesConstraintType::ChartDefault,
            HelmChartVpaType::EnabledWithChartDefault,
            true,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart.chart_path.helm_path(),
                HelmChartType::CloudProviderSpecific(KubernetesKind::Eks),
            ),
            AwsLoadBalancerControllerChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn aws_alb_controller_chart_values_file_exists_test() {
        // setup:
        let chart = AwsLoadBalancerControllerChart::new(
            None,
            "arn:aws:iam::123456789012:role/eks-alb-ingress-controller".to_string(),
            "cluster-name".to_string(),
            HelmChartResourcesConstraintType::ChartDefault,
            HelmChartVpaType::EnabledWithChartDefault,
            true,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_values_path = format!(
            "{}/lib/{}/bootstrap/chart_values/{}.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart.chart_values_path.helm_path(),
                HelmChartType::CloudProviderSpecific(KubernetesKind::Eks),
            ),
            AwsLoadBalancerControllerChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
    }

    /// Make sure rust code deosn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn aws_alb_controller_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = AwsLoadBalancerControllerChart::new(
            None,
            "arn:aws:iam::123456789012:role/eks-alb-ingress-controller".to_string(),
            "cluster-name".to_string(),
            HelmChartResourcesConstraintType::ChartDefault,
            HelmChartVpaType::EnabledWithChartDefault,
            true,
        );
        let common_chart = chart.to_common_helm_chart().unwrap();

        // execute:
        let missing_fields = get_helm_values_set_in_code_but_absent_in_values_file(
            common_chart,
            format!(
                "/lib/{}/bootstrap/chart_values/aws-load-balancer-controller.yaml",
                get_helm_path_kubernetes_provider_sub_folder_name(
                    chart.chart_values_path.helm_path(),
                    HelmChartType::CloudProviderSpecific(KubernetesKind::Eks),
                ),
            ),
        );

        // verify:
        assert!(
            missing_fields.is_none(),
            "Some fields are missing in values file, add those (make sure they still exist in chart values), fields: {}",
            missing_fields.unwrap_or_default().join(",")
        );
    }
}

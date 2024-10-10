use crate::cloud_provider::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmAction, HelmChartError, HelmChartNamespaces,
};
use crate::cloud_provider::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::errors::CommandError;
use kube::Client;

pub struct KarpenterChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    cluster_name: String,
    aws_iam_karpenter_controller_role_arn: String,
    replace_cluster_autoscaler: bool,
    enable_monitoring: bool,
    recreate_pods: bool,
}

impl KarpenterChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        cluster_name: String,
        aws_iam_karpenter_controller_role_arn: String,
        replace_cluster_autoscaler: bool,
        enable_monitoring: bool,
        recreate_pods: bool,
    ) -> Self {
        KarpenterChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                KarpenterChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                KarpenterChart::chart_name(),
            ),
            cluster_name,
            aws_iam_karpenter_controller_role_arn,
            replace_cluster_autoscaler,
            enable_monitoring,
            recreate_pods,
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
                values_files: vec![self.chart_values_path.to_string()],
                values: vec![
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
                recreate_pods: self.recreate_pods,
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
    use crate::cloud_provider::aws::kubernetes::helm_charts::karpenter::KarpenterChart;
    use crate::cloud_provider::helm_charts::{
        get_helm_path_kubernetes_provider_sub_folder_name, get_helm_values_set_in_code_but_absent_in_values_file,
        HelmChartType, ToCommonHelmChart,
    };
    use crate::cloud_provider::kubernetes::Kind as KubernetesKind;
    use std::env;
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
        assert_eq!(version.as_str(), Some("v1"));
        assert_eq!(kind.as_str(), Some("EC2NodeClass"));
    }

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn karpenter_chart_directory_exists_test() {
        // setup:
        let chart = KarpenterChart::new(None, "whatever".to_string(), "whatever".to_string(), true, true, false);

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart.chart_path.helm_path(),
                HelmChartType::CloudProviderSpecific(KubernetesKind::Eks)
            ),
            KarpenterChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn karpenter_chart_values_file_exists_test() {
        // setup:
        let chart = KarpenterChart::new(None, "whatever".to_string(), "whatever".to_string(), true, true, false);

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_values_path = format!(
            "{}/lib/{}/bootstrap/chart_values/{}.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart.chart_values_path.helm_path(),
                HelmChartType::CloudProviderSpecific(KubernetesKind::Eks)
            ),
            KarpenterChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
    }

    /// Make sure rust code doesn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn karpenter_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = KarpenterChart::new(None, "whatever".to_string(), "whatever".to_string(), true, true, false);
        let common_chart = chart.to_common_helm_chart().unwrap();

        // execute:
        let missing_fields = get_helm_values_set_in_code_but_absent_in_values_file(
            common_chart,
            format!(
                "/lib/{}/bootstrap/chart_values/{}.yaml",
                get_helm_path_kubernetes_provider_sub_folder_name(
                    chart.chart_values_path.helm_path(),
                    HelmChartType::CloudProviderSpecific(KubernetesKind::Eks)
                ),
                KarpenterChart::chart_name()
            ),
        );

        // verify:
        assert!(missing_fields.is_none(), "Some fields are missing in values file, add those (make sure they still exist in chart values), fields: {}", missing_fields.unwrap_or_default().join(","));
    }
}

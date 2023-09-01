use crate::cloud_provider::aws::regions::AwsRegion;
use crate::cloud_provider::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmChartError, HelmChartNamespaces,
};
use crate::cloud_provider::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::errors::CommandError;
use kube::Client;

pub struct ClusterAutoscalerChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    cloud_provider: String, // TODO(benjaminch): Pass cloud provider type here instead of string
    chart_image_region: AwsRegion,
    cluster_name: String,
    aws_iam_cluster_autoscaler_role_arn: String,
    prometheus_namespace: HelmChartNamespaces,
    ff_metrics_history_enabled: bool,
}

impl ClusterAutoscalerChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        cloud_provider: String,
        chart_image_region: AwsRegion,
        cluster_name: String,
        aws_iam_cluster_autoscaler_role_arn: String,
        prometheus_namespace: HelmChartNamespaces,
        ff_metrics_history_enabled: bool,
    ) -> Self {
        ClusterAutoscalerChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                ClusterAutoscalerChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                ClusterAutoscalerChart::chart_name(),
            ),
            cloud_provider,
            chart_image_region,
            cluster_name,
            aws_iam_cluster_autoscaler_role_arn,
            prometheus_namespace,
            ff_metrics_history_enabled,
        }
    }

    fn chart_name() -> String {
        "cluster-autoscaler".to_string()
    }
}

impl ToCommonHelmChart for ClusterAutoscalerChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        Ok(CommonChart {
            chart_info: ChartInfo {
                name: ClusterAutoscalerChart::chart_name(),
                path: self.chart_path.to_string(),
                values_files: vec![self.chart_values_path.to_string()],
                values: vec![
                    ChartSetValue {
                        key: "cloudProvider".to_string(),
                        value: self.cloud_provider.to_string(),
                    },
                    ChartSetValue {
                        key: "awsRegion".to_string(),
                        value: self.chart_image_region.to_aws_format().to_string(),
                    },
                    ChartSetValue {
                        key: "autoDiscovery.clusterName".to_string(),
                        value: self.cluster_name.to_string(),
                    },
                    ChartSetValue {
                        // we use string templating (r"...") to escape dot in annotation's key
                        key: r"rbac.serviceAccount.annotations.eks\.amazonaws\.com/role-arn".to_string(),
                        value: self.aws_iam_cluster_autoscaler_role_arn.to_string(),
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
                ],
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(ClusterAutoscalerChartChecker::new())),
            vertical_pod_autoscaler: None, // enabled in the chart configuration
        })
    }
}

#[derive(Clone)]
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

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::aws::kubernetes::helm_charts::cluster_autoscaler_chart::ClusterAutoscalerChart;
    use crate::cloud_provider::aws::regions::AwsRegion;
    use crate::cloud_provider::helm::HelmChartNamespaces;
    use crate::cloud_provider::helm_charts::{
        get_helm_path_kubernetes_provider_sub_folder_name, get_helm_values_set_in_code_but_absent_in_values_file,
        HelmChartType, ToCommonHelmChart,
    };
    use crate::cloud_provider::kubernetes::Kind as KubernetesKind;
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn cluster_autoscaler_chart_directory_exists_test() {
        // setup:
        let chart = ClusterAutoscalerChart::new(
            None,
            "whatever".to_string(),
            AwsRegion::EuWest3,
            "whatever".to_string(),
            "whatever".to_string(),
            HelmChartNamespaces::Prometheus,
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
                HelmChartType::CloudProviderSpecific(KubernetesKind::Eks)
            ),
            ClusterAutoscalerChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn cluster_autoscaler_chart_values_file_exists_test() {
        // setup:
        let chart = ClusterAutoscalerChart::new(
            None,
            "whatever".to_string(),
            AwsRegion::EuWest3,
            "whatever".to_string(),
            "whatever".to_string(),
            HelmChartNamespaces::Prometheus,
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
                HelmChartType::CloudProviderSpecific(KubernetesKind::Eks)
            ),
            ClusterAutoscalerChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
    }

    /// Make sure rust code doesn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn cluster_autoscaler_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = ClusterAutoscalerChart::new(
            None,
            "whatever".to_string(),
            AwsRegion::EuWest3,
            "whatever".to_string(),
            "whatever".to_string(),
            HelmChartNamespaces::Prometheus,
            true,
        );
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
                ClusterAutoscalerChart::chart_name()
            ),
        );

        // verify:
        assert!(missing_fields.is_none(), "Some fields are missing in values file, add those (make sure they still exist in chart values), fields: {}", missing_fields.unwrap_or_default().join(","));
    }
}

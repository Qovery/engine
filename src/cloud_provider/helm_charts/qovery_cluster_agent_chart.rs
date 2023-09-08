use crate::cloud_provider::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, CommonChartVpa, HelmChartError,
    HelmChartNamespaces, VpaConfig, VpaContainerPolicy, VpaTargetRef, VpaTargetRefApiVersion, VpaTargetRefKind,
};
use crate::cloud_provider::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartResources, HelmChartResourcesConstraintType,
    HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::cloud_provider::models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};
use crate::errors::CommandError;
use crate::io_models::QoveryIdentifier;
use kube::Client;
use url::Url;

pub struct QoveryClusterAgentChart {
    chart_prefix_path: Option<String>,
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    chart_resources: HelmChartResources,
    chart_image_version_tag: String,
    grpc_url: Url,
    loki_url: Option<Url>,
    cluster_jwt_token: String,
    cluster_id: QoveryIdentifier,
    organization_id: QoveryIdentifier,
    enable_vpa: bool,
}

impl QoveryClusterAgentChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        chart_image_version_tag: &str,
        grpc_url: Url,
        loki_url: Option<Url>,
        cluster_jwt_token: &str,
        cluster_id: QoveryIdentifier,
        organization_id: QoveryIdentifier,
        chart_resources: HelmChartResourcesConstraintType,
        enable_vpa: bool,
    ) -> Self {
        Self {
            chart_prefix_path: chart_prefix_path.map(|s| s.to_string()),
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                format!("qovery-{}", QoveryClusterAgentChart::chart_name()),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                format!("qovery-{}", QoveryClusterAgentChart::chart_name()),
            ),
            chart_image_version_tag: chart_image_version_tag.to_string(),
            grpc_url,
            loki_url,
            cluster_jwt_token: cluster_jwt_token.to_string(),
            cluster_id,
            organization_id,
            chart_resources: match chart_resources {
                HelmChartResourcesConstraintType::Constrained(r) => r,
                HelmChartResourcesConstraintType::ChartDefault => HelmChartResources {
                    limit_cpu: KubernetesCpuResourceUnit::MilliCpu(1000),
                    limit_memory: KubernetesMemoryResourceUnit::MebiByte(500),
                    request_cpu: KubernetesCpuResourceUnit::MilliCpu(200),
                    request_memory: KubernetesMemoryResourceUnit::MebiByte(100),
                },
            },
            enable_vpa,
        }
    }

    pub fn chart_name() -> String {
        // Should be "qovery-cluster-agent" but because of existing old release being "cluster-agent"
        // helm cannot change release name, so keep it like that for the time being
        "cluster-agent".to_string()
    }
}

impl ToCommonHelmChart for QoveryClusterAgentChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        Ok(CommonChart {
            chart_info: ChartInfo {
                name: QoveryClusterAgentChart::chart_name(),
                namespace: HelmChartNamespaces::Qovery,
                path: self.chart_path.to_string(),
                values_files: vec![self.chart_values_path.to_string()],
                values: vec![
                    ChartSetValue {
                        key: "image.tag".to_string(),
                        value: self.chart_image_version_tag.to_string(),
                    },
                    ChartSetValue {
                        key: "environmentVariables.GRPC_SERVER".to_string(),
                        value: self.grpc_url.to_string(),
                    },
                    ChartSetValue {
                        key: "environmentVariables.LOKI_URL".to_string(),
                        value: match &self.loki_url {
                            // If log history is enabled, add the loki url to the values
                            Some(loki_url) => loki_url.to_string(),
                            None => "".to_string(), // empty value is handled by the chart
                        },
                    },
                    ChartSetValue {
                        key: "environmentVariables.CLUSTER_JWT_TOKEN".to_string(),
                        value: self.cluster_jwt_token.to_string(),
                    },
                    ChartSetValue {
                        key: "environmentVariables.CLUSTER_ID".to_string(),
                        value: self.cluster_id.to_string(),
                    },
                    ChartSetValue {
                        key: "environmentVariables.ORGANIZATION_ID".to_string(),
                        value: self.organization_id.to_string(),
                    },
                    // Resources
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
            chart_installation_checker: Some(Box::new(QoveryClusterAgentChartChecker::new())),
            vertical_pod_autoscaler: match self.enable_vpa {
                true => Some(CommonChartVpa::new(
                    self.chart_prefix_path.clone().unwrap_or(".".to_string()),
                    vec![VpaConfig {
                        target_ref: VpaTargetRef::new(
                            VpaTargetRefApiVersion::AppsV1,
                            VpaTargetRefKind::Deployment,
                            "qovery-cluster-agent".to_string(),
                        ),
                        container_policy: VpaContainerPolicy::new(
                            "*".to_string(),
                            Some(KubernetesCpuResourceUnit::MilliCpu(150)),
                            Some(KubernetesCpuResourceUnit::MilliCpu(500)),
                            Some(KubernetesMemoryResourceUnit::MebiByte(64)),
                            Some(KubernetesMemoryResourceUnit::GibiByte(1)),
                        ),
                    }],
                )),
                false => None,
            },
        })
    }
}

#[derive(Clone)]
pub struct QoveryClusterAgentChartChecker {}

impl QoveryClusterAgentChartChecker {
    pub fn new() -> QoveryClusterAgentChartChecker {
        QoveryClusterAgentChartChecker {}
    }
}

impl Default for QoveryClusterAgentChartChecker {
    fn default() -> Self {
        QoveryClusterAgentChartChecker::new()
    }
}

impl ChartInstallationChecker for QoveryClusterAgentChartChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO(ENG-1557): Implement chart install verification
        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::helm_charts::qovery_cluster_agent_chart::QoveryClusterAgentChart;
    use crate::cloud_provider::helm_charts::{
        get_helm_path_kubernetes_provider_sub_folder_name, HelmChartResourcesConstraintType, HelmChartType,
    };
    use crate::io_models::QoveryIdentifier;
    use std::env;
    use url::Url;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn qovery_cluster_agent_chart_directory_exists_test() {
        // setup:
        let chart = QoveryClusterAgentChart::new(
            None,
            "image-tag",
            Url::parse("http://grpc.qovery.com:443").expect("cannot parse GRPC url"),
            Some(Url::parse("http://loki.logging.svc.cluster.local:3100").expect("cannot parse Loki url")),
            "a_jwt_token",
            QoveryIdentifier::new_random(),
            QoveryIdentifier::new_random(),
            HelmChartResourcesConstraintType::ChartDefault,
            false,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/qovery-{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared,),
            QoveryClusterAgentChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn qovery_cluster_agent_chart_values_file_exists_test() {
        // setup:
        let chart = QoveryClusterAgentChart::new(
            None,
            "image-tag",
            Url::parse("http://grpc.qovery.com:443").expect("cannot parse GRPC url"),
            Some(Url::parse("http://loki.logging.svc.cluster.local:3100").expect("cannot parse Loki url")),
            "a_jwt_token",
            QoveryIdentifier::new_random(),
            QoveryIdentifier::new_random(),
            HelmChartResourcesConstraintType::ChartDefault,
            false,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_values_path = format!(
            "{}/lib/{}/bootstrap/chart_values/qovery-{}.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart.chart_values_path.helm_path(),
                HelmChartType::Shared,
            ),
            QoveryClusterAgentChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
    }

    // Make sure rust code deosn't set a value not declared inside values file.
    // All values should be declared / set in values file unless it needs to be injected via rust code.
    // #[test]
    // fn qovery_cluster_agent_chart_rust_overridden_values_exists_in_values_yaml_test() {
    //     // setup:
    //     let chart = QoveryClusterAgentChart::new(
    //         None,
    //         "image-tag",
    //         Url::parse("http://grpc.qovery.com:443").expect("cannot parse GRPC url"),
    //         Some(Url::parse("http://loki.logging.svc.cluster.local:3100").expect("cannot parse Loki url")),
    //         "a_jwt_token",
    //         QoveryIdentifier::new_random(),
    //         QoveryIdentifier::new_random(),
    //         HelmChartResourcesConstraintType::ChartDefault,
    //         false,
    //     );
    //     let common_chart = chart.to_common_helm_chart().unwrap();

    //     // execute:
    //     let missing_fields = get_helm_values_set_in_code_but_absent_in_values_file(
    //         common_chart,
    //         format!(
    //             "/lib/{}/bootstrap/chart_values/qovery-{}.yaml",
    //             get_helm_path_kubernetes_provider_sub_folder_name(
    //                 chart.chart_values_path.helm_path(),
    //                 HelmChartType::Shared,
    //             ),
    //             QoveryClusterAgentChart::chart_name(),
    //         ),
    //     );

    //     // verify:
    //     assert!(missing_fields.is_none(), "Some fields are missing in values file, add those (make sure they still exist in chart values), fields: {}", missing_fields.unwrap_or_default().join(","));
    // }
}

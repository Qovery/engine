use crate::cloud_provider::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmChartError, HelmChartNamespaces,
    UpdateStrategy,
};
use crate::cloud_provider::helm_charts::{HelmChartDirectoryLocation, HelmChartPath, ToCommonHelmChart};
use crate::cloud_provider::models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};
use crate::errors::CommandError;
use crate::io_models::QoveryIdentifier;
use kube::Client;

use super::{HelmChartResources, HelmChartResourcesConstraintType};

pub struct QoveryShellAgentChart {
    chart_path: HelmChartPath,
    cluster_jwt_token: String,
    chart_image_version_tag: String,
    grpc_url: String,
    cluster_id: QoveryIdentifier,
    organization_long_id: QoveryIdentifier,
    chart_resources: HelmChartResourcesConstraintType,
    update_strategy: UpdateStrategy,
}

impl QoveryShellAgentChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        chart_image_version_tag: &str,
        cluster_jwt_token: String,
        organization_long_id: QoveryIdentifier,
        cluster_id: QoveryIdentifier,
        grpc_url: String,
        chart_resources: HelmChartResourcesConstraintType,
        update_strategy: UpdateStrategy,
    ) -> Self {
        Self {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                format!("qovery-{}", QoveryShellAgentChart::chart_name()),
            ),
            cluster_jwt_token,
            chart_image_version_tag: chart_image_version_tag.to_string(),
            grpc_url,
            cluster_id,
            organization_long_id,
            chart_resources,
            update_strategy,
        }
    }

    pub fn chart_name() -> String {
        // Should be "qovery-cluster-agent" but because of existing old release being "cluster-agent"
        // helm cannot change release name, so keep it like that for the time being
        "shell-agent".to_string()
    }
}

impl ToCommonHelmChart for QoveryShellAgentChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        let resources = match &self.chart_resources {
            HelmChartResourcesConstraintType::ChartDefault => &HelmChartResources {
                limit_cpu: KubernetesCpuResourceUnit::MilliCpu(1000),
                limit_memory: KubernetesMemoryResourceUnit::MebiByte(100),
                request_cpu: KubernetesCpuResourceUnit::MilliCpu(200),
                request_memory: KubernetesMemoryResourceUnit::MebiByte(100),
            },
            HelmChartResourcesConstraintType::Constrained(x) => x,
        };

        Ok(CommonChart {
            chart_info: ChartInfo {
                name: QoveryShellAgentChart::chart_name(),
                namespace: HelmChartNamespaces::Qovery,
                path: self.chart_path.to_string(),
                values: vec![
                    ChartSetValue {
                        key: "image.tag".to_string(),
                        value: self.chart_image_version_tag.to_string(),
                    },
                    ChartSetValue {
                        key: "replicaCount".to_string(),
                        value: "1".to_string(),
                    },
                    ChartSetValue {
                        key: "rolloutStrategy".to_string(),
                        value: self.update_strategy.to_string(),
                    },
                    ChartSetValue {
                        key: "environmentVariables.RUST_BACKTRACE".to_string(),
                        value: "full".to_string(),
                    },
                    ChartSetValue {
                        key: "environmentVariables.RUST_LOG".to_string(),
                        value: "h2::codec::framed_write=INFO\\,INFO".to_string(),
                    },
                    ChartSetValue {
                        key: "environmentVariables.GRPC_SERVER".to_string(),
                        value: self.grpc_url.to_string(),
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
                        value: self.organization_long_id.to_string(),
                    },
                    ChartSetValue {
                        key: "resources.limits.cpu".to_string(),
                        value: resources.limit_cpu.to_string(),
                    },
                    ChartSetValue {
                        key: "resources.requests.cpu".to_string(),
                        value: resources.request_cpu.to_string(),
                    },
                    ChartSetValue {
                        key: "resources.limits.memory".to_string(),
                        value: resources.limit_memory.to_string(),
                    },
                    ChartSetValue {
                        key: "resources.requests.memory".to_string(),
                        value: resources.request_memory.to_string(),
                    },
                ],
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(QoveryShellAgentChartChecker::new())),
            vertical_pod_autoscaler: None,
        })
    }
}

#[derive(Clone)]
pub struct QoveryShellAgentChartChecker {}

impl QoveryShellAgentChartChecker {
    pub fn new() -> QoveryShellAgentChartChecker {
        QoveryShellAgentChartChecker {}
    }
}

impl Default for QoveryShellAgentChartChecker {
    fn default() -> Self {
        QoveryShellAgentChartChecker::new()
    }
}

impl ChartInstallationChecker for QoveryShellAgentChartChecker {
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
    use crate::cloud_provider::helm::UpdateStrategy;
    use crate::cloud_provider::helm_charts::qovery_shell_agent_chart::QoveryShellAgentChart;
    use crate::cloud_provider::helm_charts::{
        get_helm_path_kubernetes_provider_sub_folder_name, HelmChartResourcesConstraintType, HelmChartType,
    };
    use crate::io_models::QoveryIdentifier;
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn qovery_cluster_agent_chart_directory_exists_test() {
        // setup:
        let chart = QoveryShellAgentChart::new(
            None,
            "image-tag",
            "a_jwt_token".to_string(),
            QoveryIdentifier::new_random(),
            QoveryIdentifier::new_random(),
            "http://grpc.qovery.com:443".to_string(),
            HelmChartResourcesConstraintType::ChartDefault,
            UpdateStrategy::RollingUpdate,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/qovery-{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared,),
            QoveryShellAgentChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }
}

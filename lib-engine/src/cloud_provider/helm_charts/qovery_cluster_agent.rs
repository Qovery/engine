use crate::cloud_provider::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, CommonChartVpa, HelmChartError,
    HelmChartNamespaces, VpaConfig, VpaContainerPolicy, VpaTargetRef, VpaTargetRefApiVersion, VpaTargetRefKind,
};
use crate::cloud_provider::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::cloud_provider::models::KubernetesMemoryResourceUnit;
use crate::errors::CommandError;
use kube::Client;
use uuid::Uuid;

pub struct QoveryClusterAgentChart {
    chart_prefix_path: Option<String>,
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    version: String,
    api_url: String,
    organization_long_id: Uuid,
    cluster_id: String,
    cluster_long_id: Uuid,
    cluster_jwt_token: String,
    grpc_url: String,
    loki_url: Option<String>,
    enable_vpa: bool,
}

impl QoveryClusterAgentChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        chart_path: HelmChartPath,
        chart_values_path: HelmChartValuesFilePath,
        version: String,
        api_url: String,
        organization_long_id: Uuid,
        cluster_id: String,
        cluster_long_id: Uuid,
        cluster_jwt_token: String,
        grpc_url: String,
        loki_url: Option<String>,
        enable_vpa: bool,
    ) -> QoveryClusterAgentChart {
        QoveryClusterAgentChart {
            chart_prefix_path: chart_prefix_path.map(|s| s.to_string()),
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                QoveryClusterAgentChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                QoveryClusterAgentChart::chart_name(),
            ),
            version,
            api_url,
            organization_long_id,
            cluster_id,
            cluster_long_id,
            cluster_jwt_token,
            grpc_url,
            loki_url,
            enable_vpa,
        }
    }

    fn chart_name() -> String {
        "external-dns".to_string()
    }
}

impl ToCommonHelmChart for QoveryClusterAgentChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        let mut values = vec![
            ChartSetValue {
                key: "image.tag".to_string(),
                value: self.version.to_string(),
            },
            ChartSetValue {
                key: "replicaCount".to_string(),
                value: "1".to_string(),
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
                value: self.cluster_long_id.to_string(),
            },
            ChartSetValue {
                key: "environmentVariables.ORGANIZATION_ID".to_string(),
                value: self.organization_long_id.to_string(),
            },
            ChartSetValue {
                key: "resources.requests.cpu".to_string(),
                value: "200m".to_string(),
            },
            ChartSetValue {
                key: "resources.limits.cpu".to_string(),
                value: "1".to_string(),
            },
            ChartSetValue {
                key: "resources.requests.memory".to_string(),
                value: "100Mi".to_string(),
            },
            ChartSetValue {
                key: "resources.limits.memory".to_string(),
                value: "500Mi".to_string(),
            },
        ];

        // If log history is enabled, add the loki url to the values
        if let Some(url) = self.loki_url.clone() {
            values.push(ChartSetValue {
                key: "environmentVariables.LOKI_URL".to_string(),
                value: url,
            });
        }

        Ok(CommonChart {
            chart_info: ChartInfo {
                name: "cluster-agent".to_string(),
                path: self.chart_path.to_string(),
                namespace: HelmChartNamespaces::Qovery,
                values,
                ..Default::default()
            },
            vertical_pod_autoscaler: match self.enable_vpa {
                true => Some(CommonChartVpa::new(
                    self.chart_prefix_path.clone().unwrap_or(".".to_string()),
                    vec![VpaConfig {
                        target_ref: VpaTargetRef::new(
                            VpaTargetRefApiVersion::AppsV1,
                            VpaTargetRefKind::Deployment,
                            "shell-agent-qovery-shell-agent".to_string(),
                        ),
                        container_policy: VpaContainerPolicy::new(
                            "*".to_string(),
                            None,
                            None,
                            Some(KubernetesMemoryResourceUnit::MebiByte(32)),
                            Some(KubernetesMemoryResourceUnit::MebiByte(256)),
                        ),
                    }],
                )),
                false => None,
            },
            ..Default::default()
        })
    }
}

#[derive(Clone)]
struct QoveryClusterAgentChartInstallationChecker {}

impl QoveryClusterAgentChartInstallationChecker {
    pub fn new() -> Self {
        QoveryClusterAgentChartInstallationChecker {}
    }
}

impl Default for QoveryClusterAgentChartInstallationChecker {
    fn default() -> Self {
        QoveryClusterAgentChartInstallationChecker::new()
    }
}

impl ChartInstallationChecker for QoveryClusterAgentChartInstallationChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO(ENG-1368): Implement chart install verification
        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

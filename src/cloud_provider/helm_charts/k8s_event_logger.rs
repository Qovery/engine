use crate::cloud_provider::helm::{
    ChartInfo, ChartSetValue, CommonChart, CommonChartVpa, HelmChartError, HelmChartNamespaces, VpaConfig,
    VpaContainerPolicy, VpaTargetRef, VpaTargetRefApiVersion, VpaTargetRefKind,
};
use crate::cloud_provider::helm_charts::{HelmChartDirectoryLocation, HelmChartPath, ToCommonHelmChart};
use crate::cloud_provider::models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};

pub struct K8sEventLoggerChart {
    chart_prefix_path: Option<String>,
    chart_path: HelmChartPath,
    enable_vpa: bool,
    namespace: HelmChartNamespaces,
}

impl K8sEventLoggerChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        enable_vpa: bool,
        namespace: HelmChartNamespaces,
    ) -> K8sEventLoggerChart {
        K8sEventLoggerChart {
            chart_prefix_path: chart_prefix_path.map(|s| s.to_string()),
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                K8sEventLoggerChart::chart_name(),
            ),
            enable_vpa,
            namespace,
        }
    }

    fn chart_name() -> String {
        "k8s-event-logger".to_string()
    }
}

impl ToCommonHelmChart for K8sEventLoggerChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        Ok(CommonChart {
            chart_info: ChartInfo {
                name: "k8s-event-logger".to_string(),
                namespace: self.namespace,
                path: self.chart_path.to_string(),
                values: vec![
                    ChartSetValue {
                        key: "image.repository".to_string(),
                        value: "public.ecr.aws/r3m4q3r9/pub-mirror-k8s-event-logger".to_string(),
                    },
                    ChartSetValue {
                        key: "resources.limits.cpu".to_string(),
                        value: KubernetesCpuResourceUnit::MilliCpu(500).to_string(),
                    },
                    ChartSetValue {
                        key: "resources.limits.memory".to_string(),
                        value: KubernetesMemoryResourceUnit::MebiByte(384).to_string(),
                    },
                    ChartSetValue {
                        key: "resources.requests.cpu".to_string(),
                        value: KubernetesCpuResourceUnit::MilliCpu(50).to_string(),
                    },
                    ChartSetValue {
                        key: "resources.requests.memory".to_string(),
                        value: KubernetesMemoryResourceUnit::MebiByte(32).to_string(),
                    },
                ],
                ..Default::default()
            },
            vertical_pod_autoscaler: match self.enable_vpa {
                true => Some(CommonChartVpa::new(
                    self.chart_prefix_path.clone().unwrap_or(".".to_string()),
                    vec![VpaConfig {
                        target_ref: VpaTargetRef::new(
                            VpaTargetRefApiVersion::AppsV1,
                            VpaTargetRefKind::Deployment,
                            "k8s-event-logger".to_string(),
                        ),
                        container_policy: VpaContainerPolicy::new(
                            "*".to_string(),
                            Some(KubernetesCpuResourceUnit::MilliCpu(50)),
                            Some(KubernetesCpuResourceUnit::MilliCpu(500)),
                            Some(KubernetesMemoryResourceUnit::MebiByte(32)),
                            Some(KubernetesMemoryResourceUnit::MebiByte(256)),
                        ),
                    }],
                )),
                false => None,
            },
            chart_installation_checker: None,
        })
    }
}

use crate::helm::{
    ChartInfo, CommonChart, CommonChartVpa, HelmChartError, HelmChartNamespaces, VpaConfig, VpaContainerPolicy,
    VpaTargetRef, VpaTargetRefApiVersion, VpaTargetRefKind,
};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::io_models::models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};

pub struct K8sEventLoggerChart {
    chart_prefix_path: Option<String>,
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    enable_vpa: bool,
    namespace: HelmChartNamespaces,
    additional_chart_values: Vec<HelmChartValuesFilePath>,
}

impl K8sEventLoggerChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        enable_vpa: bool,
        namespace: HelmChartNamespaces,
        metrics_enabled: bool,
    ) -> K8sEventLoggerChart {
        let mut additional_chart_values = vec![];

        if metrics_enabled {
            additional_chart_values.push(HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                "k8s-event-logger-with-prometheus".to_string(),
            ));
        }

        K8sEventLoggerChart {
            chart_prefix_path: chart_prefix_path.map(|s| s.to_string()),
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                K8sEventLoggerChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                K8sEventLoggerChart::chart_name(),
            ),
            enable_vpa,
            namespace,
            additional_chart_values,
        }
    }

    fn chart_name() -> String {
        "k8s-event-logger".to_string()
    }
}

impl ToCommonHelmChart for K8sEventLoggerChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        let values_files: Vec<String> = std::iter::once(&self.chart_values_path)
            .chain(self.additional_chart_values.iter())
            .map(ToString::to_string)
            .collect();

        Ok(CommonChart {
            chart_info: ChartInfo {
                name: "k8s-event-logger".to_string(),
                namespace: self.namespace,
                path: self.chart_path.to_string(),
                values_files,
                values: vec![],
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

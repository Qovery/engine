use std::sync::Arc;

use crate::cloud_provider::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, CommonChartVpa, HelmChartError,
    HelmChartNamespaces, VpaConfig, VpaContainerPolicy, VpaTargetRef, VpaTargetRefApiVersion, VpaTargetRefKind,
};
use crate::cloud_provider::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::cloud_provider::models::{
    CustomerHelmChartsOverride, KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit,
};
use crate::cloud_provider::Kind;
use crate::cmd::helm_utils::CRDSUpdate;
use crate::errors::CommandError;
use kube::Client;
use semver::Version;

pub type StorageClassName = String;

pub struct KubePrometheusStackChart {
    chart_prefix_path: Option<String>,
    chart_path: HelmChartPath,
    chart_values_paths: Vec<HelmChartValuesFilePath>,
    storage_class_name: StorageClassName,
    prometheus_internal_url: String,
    prometheus_namespace: HelmChartNamespaces,
    kubelet_service_monitor_resource_enabled: bool,
    customer_helm_chart_override: Option<CustomerHelmChartsOverride>,
    enable_vpa: bool,
}

impl KubePrometheusStackChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        storage_class_name: StorageClassName,
        prometheus_internal_url: String,
        prometheus_namespace: HelmChartNamespaces,
        kubelet_service_monitor_resource_enabled: bool,
        customer_helm_chart_fn: Arc<dyn Fn(String) -> Option<CustomerHelmChartsOverride>>,
        enable_vpa: bool,
        provider_kind: Kind,
    ) -> Self {
        let mut values_locations = vec![HelmChartDirectoryLocation::CommonFolder];
        if provider_kind == Kind::Aws {
            values_locations.push(HelmChartDirectoryLocation::CloudProviderFolder)
        }

        KubePrometheusStackChart {
            chart_prefix_path: chart_prefix_path.map(|s| s.to_string()),
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                KubePrometheusStackChart::chart_name(),
            ),
            chart_values_paths: values_locations
                .into_iter()
                .map(|values_location| {
                    HelmChartValuesFilePath::new(
                        chart_prefix_path,
                        values_location,
                        KubePrometheusStackChart::chart_name(),
                    )
                })
                .collect(),
            storage_class_name,
            prometheus_internal_url,
            prometheus_namespace,
            kubelet_service_monitor_resource_enabled,
            customer_helm_chart_override: customer_helm_chart_fn(Self::chart_name()),
            enable_vpa,
        }
    }

    pub fn chart_name() -> String {
        "kube-prometheus-stack".to_string()
    }
}

impl ToCommonHelmChart for KubePrometheusStackChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        Ok(CommonChart {
            chart_info: ChartInfo {
                name: KubePrometheusStackChart::chart_name(),
                path: self.chart_path.to_string(),
                namespace: self.prometheus_namespace,
                reinstall_chart_if_installed_version_is_below_than: Some(Version::new(51, 0, 2)),
                // high timeout because on bootstrap, it's one of the biggest dependencies and on upgrade, it can takes time
                // to upgrade because of the CRD and the number of elements it has to deploy
                timeout_in_seconds: 480,
                // To check for upgrades: https://github.com/prometheus-community/helm-charts/tree/main/charts/kube-prometheus-stack
                crds_update: Some(CRDSUpdate{
                    path:"https://raw.githubusercontent.com/prometheus-operator/prometheus-operator/v0.68.0/example/prometheus-operator-crd".to_string(),
                    resources: vec![
                        "monitoring.coreos.com_alertmanagerconfigs.yaml".to_string(),
                        "monitoring.coreos.com_alertmanagers.yaml".to_string(),
                        "monitoring.coreos.com_podmonitors.yaml".to_string(),
                        "monitoring.coreos.com_probes.yaml".to_string(),
                        "monitoring.coreos.com_prometheusagents.yaml".to_string(),
                        "monitoring.coreos.com_prometheuses.yaml".to_string(),
                        "monitoring.coreos.com_prometheusrules.yaml".to_string(),
                        "monitoring.coreos.com_scrapeconfigs.yaml".to_string(),
                        "monitoring.coreos.com_servicemonitors.yaml".to_string(),
                        "monitoring.coreos.com_thanosrulers.yaml".to_string(),
                    ]
                }),
                values_files: self.chart_values_paths.iter().map(|chart_values_path| chart_values_path.to_string()).collect(),
                values: vec![
                    ChartSetValue {
                        key: "prometheus.prometheusSpec.storageSpec.volumeClaimTemplate.spec.storageClassName".to_string(),
                        value: self.storage_class_name.to_string(),
                    },
                    ChartSetValue {
                        key: "prometheus.prometheusSpec.externalUrl".to_string(),
                        value: self.prometheus_internal_url.clone(),
                    },
                    ChartSetValue {
                        key: "kubelet.serviceMonitor.resource".to_string(),
                        value: self.kubelet_service_monitor_resource_enabled.to_string(),
                    },
                ],
                yaml_files_content: match self.customer_helm_chart_override.clone() {
                    Some(x) => vec![x.to_chart_values_generated()],
                    None => vec![],
                },
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(KubePrometheusStackChartChecker::new())),
            vertical_pod_autoscaler: match self.enable_vpa {
                true => Some(CommonChartVpa::new(
                    self.chart_prefix_path.clone().unwrap_or(".".to_string()),
                    vec![
                    VpaConfig {
                        target_ref: VpaTargetRef::new(
                            VpaTargetRefApiVersion::AppsV1,
                            VpaTargetRefKind::Deployment,
                            "kube-prometheus-stack-operator".to_string(),
                        ),
                        container_policy: VpaContainerPolicy::new(
                            "*".to_string(),
                            Some(KubernetesCpuResourceUnit::MilliCpu(200)),
                            Some(KubernetesCpuResourceUnit::MilliCpu(2000)),
                            Some(KubernetesMemoryResourceUnit::MebiByte(384)),
                            Some(KubernetesMemoryResourceUnit::GibiByte(4)),
                        ),
                    },
                    VpaConfig {
                        target_ref: VpaTargetRef::new(
                            VpaTargetRefApiVersion::AppsV1,
                            VpaTargetRefKind::Deployment,
                            "kube-state-metrics".to_string(),
                        ),
                        container_policy: VpaContainerPolicy::new(
                            "*".to_string(),
                            Some(KubernetesCpuResourceUnit::MilliCpu(50)),
                            Some(KubernetesCpuResourceUnit::MilliCpu(200)),
                            Some(KubernetesMemoryResourceUnit::MebiByte(64)),
                            Some(KubernetesMemoryResourceUnit::GibiByte(1)),
                        ),
                    },
                    VpaConfig {
                        target_ref: VpaTargetRef::new(
                            VpaTargetRefApiVersion::AppsV1,
                            VpaTargetRefKind::DaemonSet,
                            "kube-prometheus-stack-prometheus-node-exporter".to_string(),
                        ),
                        container_policy: VpaContainerPolicy::new(
                            "*".to_string(),
                            Some(KubernetesCpuResourceUnit::MilliCpu(150)),
                            Some(KubernetesCpuResourceUnit::MilliCpu(500)),
                            Some(KubernetesMemoryResourceUnit::MebiByte(16)),
                            Some(KubernetesMemoryResourceUnit::MebiByte(256)),
                        ),
                    },
                    VpaConfig {
                        target_ref: VpaTargetRef::new(
                            VpaTargetRefApiVersion::AppsV1,
                            VpaTargetRefKind::StatefulSet,
                            "prometheus-kube-prometheus-stack-prometheus".to_string(),
                        ),
                        container_policy: VpaContainerPolicy::new(
                            "*".to_string(),
                            Some(KubernetesCpuResourceUnit::MilliCpu(100)),
                            Some(KubernetesCpuResourceUnit::MilliCpu(1000)),
                            Some(KubernetesMemoryResourceUnit::GibiByte(1)),
                            Some(KubernetesMemoryResourceUnit::GibiByte(8)),
                        ),
                    },
                    ],
                )),
                false => None,
            },
        })
    }
}

#[derive(Clone)]
pub struct KubePrometheusStackChartChecker {}

impl KubePrometheusStackChartChecker {
    pub fn new() -> KubePrometheusStackChartChecker {
        KubePrometheusStackChartChecker {}
    }
}

impl Default for KubePrometheusStackChartChecker {
    fn default() -> Self {
        KubePrometheusStackChartChecker::new()
    }
}

impl ChartInstallationChecker for KubePrometheusStackChartChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO(ENG-1373): Implement chart install verification
        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::helm::HelmChartNamespaces;
    use crate::cloud_provider::helm_charts::kube_prometheus_stack_chart::{KubePrometheusStackChart, StorageClassName};
    use crate::cloud_provider::helm_charts::{
        get_helm_path_kubernetes_provider_sub_folder_name, get_helm_values_set_in_code_but_absent_in_values_file,
        HelmChartType, ToCommonHelmChart,
    };
    use crate::cloud_provider::models::CustomerHelmChartsOverride;
    use crate::cloud_provider::Kind;
    use std::env;
    use std::sync::Arc;

    fn get_prometheus_chart_override() -> Arc<dyn Fn(String) -> Option<CustomerHelmChartsOverride>> {
        Arc::new(|_chart_name: String| -> Option<CustomerHelmChartsOverride> {
            Some(CustomerHelmChartsOverride {
                chart_name: KubePrometheusStackChart::chart_name(),
                chart_values: "".to_string(),
            })
        })
    }
    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn kube_prometheus_stack_chart_directory_exists_test() {
        // setup:
        let chart = KubePrometheusStackChart::new(
            None,
            StorageClassName::new(),
            "whatever".to_string(),
            HelmChartNamespaces::Prometheus,
            true,
            get_prometheus_chart_override(),
            false,
            Kind::SelfManaged,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared),
            KubePrometheusStackChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn kube_prometheus_stack_chart_values_file_exists_test() {
        // setup:
        let chart = KubePrometheusStackChart::new(
            None,
            StorageClassName::new(),
            "whatever".to_string(),
            HelmChartNamespaces::Prometheus,
            true,
            get_prometheus_chart_override(),
            false,
            Kind::Gcp,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        for chart_values_path in chart.chart_values_paths {
            let chart_values_path = format!(
                "{}/lib/{}/bootstrap/chart_values/{}.yaml",
                current_directory
                    .to_str()
                    .expect("Impossible to convert current directory to string"),
                get_helm_path_kubernetes_provider_sub_folder_name(chart_values_path.helm_path(), HelmChartType::Shared),
                KubePrometheusStackChart::chart_name(),
            );

            // execute
            let values_file = std::fs::File::open(&chart_values_path);

            // verify:
            assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
        }
    }

    /// Make sure rust code doesn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn kube_prometheus_stack_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = KubePrometheusStackChart::new(
            None,
            "whatever".to_string(),
            "whatever".to_string(),
            HelmChartNamespaces::Prometheus,
            true,
            get_prometheus_chart_override(),
            false,
            Kind::Scw,
        );
        let common_chart = chart.to_common_helm_chart().unwrap();

        // execute:
        for chart_values_path in chart.chart_values_paths {
            let missing_fields = get_helm_values_set_in_code_but_absent_in_values_file(
                common_chart.clone(),
                format!(
                    "/lib/{}/bootstrap/chart_values/{}.yaml",
                    get_helm_path_kubernetes_provider_sub_folder_name(
                        chart_values_path.helm_path(),
                        HelmChartType::Shared
                    ),
                    KubePrometheusStackChart::chart_name()
                ),
            );

            // verify:
            assert!(missing_fields.is_none(), "Some fields are missing in values file, add those (make sure they still exist in chart values), fields: {}", missing_fields.unwrap_or_default().join(","));
        }
    }
}

use std::sync::Arc;

use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, CommonChartVpa, HelmChartError,
    HelmChartNamespaces, UpdateStrategy, VpaConfig, VpaContainerPolicy, VpaTargetRef, VpaTargetRefApiVersion,
    VpaTargetRefKind,
};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartResources, HelmChartResourcesConstraintType,
    HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::io_models::models::CustomerHelmChartsOverride;
use crate::io_models::models::{KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};
use kube::Client;
use semver::Version;

pub struct CertManagerChart {
    chart_prefix_path: Option<String>,
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    ff_metrics_history_enabled: bool,
    chart_resources: HelmChartResources,
    webhook_resources: HelmChartResources,
    ca_injector_resources: HelmChartResources,
    update_strategy: UpdateStrategy,
    customer_helm_chart_override: Option<CustomerHelmChartsOverride>,
    enable_vpa: bool,
    namespace: HelmChartNamespaces,
    leader_election_namespace: HelmChartNamespaces,
}

impl CertManagerChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        ff_metrics_history_enabled: bool,
        chart_resources: HelmChartResourcesConstraintType,
        webhook_resources: HelmChartResourcesConstraintType,
        ca_injector_resources: HelmChartResourcesConstraintType,
        update_strategy: UpdateStrategy,
        customer_helm_chart_fn: Arc<dyn Fn(String) -> Option<CustomerHelmChartsOverride>>,
        enable_vpa: bool,
        namespace: HelmChartNamespaces,
        leader_election_namespace: HelmChartNamespaces,
    ) -> CertManagerChart {
        CertManagerChart {
            chart_prefix_path: chart_prefix_path.map(|s| s.to_string()),
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                CertManagerChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                CertManagerChart::chart_name(),
            ),
            chart_resources: match chart_resources {
                HelmChartResourcesConstraintType::ChartDefault => HelmChartResources {
                    request_cpu: KubernetesCpuResourceUnit::MilliCpu(100),
                    request_memory: KubernetesMemoryResourceUnit::GibiByte(1),
                    limit_cpu: KubernetesCpuResourceUnit::MilliCpu(200),
                    limit_memory: KubernetesMemoryResourceUnit::GibiByte(1),
                },
                HelmChartResourcesConstraintType::Constrained(r) => r,
            },
            webhook_resources: match webhook_resources {
                HelmChartResourcesConstraintType::ChartDefault => HelmChartResources {
                    request_cpu: KubernetesCpuResourceUnit::MilliCpu(50),
                    request_memory: KubernetesMemoryResourceUnit::MebiByte(128),
                    limit_cpu: KubernetesCpuResourceUnit::MilliCpu(200),
                    limit_memory: KubernetesMemoryResourceUnit::MebiByte(128),
                },
                HelmChartResourcesConstraintType::Constrained(r) => r,
            },
            ca_injector_resources: match ca_injector_resources {
                HelmChartResourcesConstraintType::ChartDefault => HelmChartResources {
                    request_cpu: KubernetesCpuResourceUnit::MilliCpu(100),
                    request_memory: KubernetesMemoryResourceUnit::GibiByte(1),
                    limit_cpu: KubernetesCpuResourceUnit::MilliCpu(500),
                    limit_memory: KubernetesMemoryResourceUnit::GibiByte(1),
                },
                HelmChartResourcesConstraintType::Constrained(r) => r,
            },
            ff_metrics_history_enabled,
            update_strategy,
            customer_helm_chart_override: customer_helm_chart_fn(Self::chart_name()),
            enable_vpa,
            namespace,
            leader_election_namespace,
        }
    }

    pub fn chart_name() -> String {
        "cert-manager".to_string()
    }
}

impl ToCommonHelmChart for CertManagerChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        Ok(CommonChart {
            chart_info: ChartInfo {
                name: CertManagerChart::chart_name(),
                path: self.chart_path.to_string(),
                namespace: self.namespace.clone(),
                reinstall_chart_if_installed_version_is_below_than: Some(Version::new(1, 4, 4)),
                values_files: vec![self.chart_values_path.to_string()],
                values: vec![
                    ChartSetValue {
                        key: "global.leaderElection.namespace".to_string(),
                        value: self.leader_election_namespace.to_string(),
                    },
                    ChartSetValue {
                        key: "strategy.type".to_string(),
                        value: self.update_strategy.to_string(),
                    },
                    // https://cert-manager.io/docs/configuration/acme/dns01/#setting-nameservers-for-dns01-self-check
                    ChartSetValue {
                        key: "prometheus.servicemonitor.enabled".to_string(),
                        value: self.ff_metrics_history_enabled.to_string(),
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
                    // Webhooks
                    ChartSetValue {
                        key: "webhook.strategy.type".to_string(),
                        value: self.update_strategy.to_string(),
                    },
                    ChartSetValue {
                        key: "webhook.resources.limits.cpu".to_string(),
                        value: self.webhook_resources.limit_cpu.to_string(),
                    },
                    ChartSetValue {
                        key: "webhook.resources.limits.memory".to_string(),
                        value: self.webhook_resources.limit_memory.to_string(),
                    },
                    ChartSetValue {
                        key: "webhook.resources.requests.cpu".to_string(),
                        value: self.webhook_resources.request_cpu.to_string(),
                    },
                    ChartSetValue {
                        key: "webhook.resources.requests.memory".to_string(),
                        value: self.webhook_resources.request_memory.to_string(),
                    },
                    // Cainjector
                    ChartSetValue {
                        key: "cainjector.strategy.type".to_string(),
                        value: self.update_strategy.to_string(),
                    },
                    ChartSetValue {
                        key: "cainjector.resources.limits.cpu".to_string(),
                        value: self.ca_injector_resources.limit_cpu.to_string(),
                    },
                    ChartSetValue {
                        key: "cainjector.resources.limits.memory".to_string(),
                        value: self.ca_injector_resources.limit_memory.to_string(),
                    },
                    ChartSetValue {
                        key: "cainjector.resources.requests.cpu".to_string(),
                        value: self.ca_injector_resources.request_cpu.to_string(),
                    },
                    ChartSetValue {
                        key: "cainjector.resources.requests.memory".to_string(),
                        value: self.ca_injector_resources.request_memory.to_string(),
                    },
                ],
                yaml_files_content: match self.customer_helm_chart_override.clone() {
                    Some(x) => vec![x.to_chart_values_generated()],
                    None => vec![],
                },
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(CertManagerChartChecker::new())),
            vertical_pod_autoscaler: match self.enable_vpa {
                true => Some(CommonChartVpa::new(
                    self.chart_prefix_path.clone().unwrap_or(".".to_string()),
                    vec![
                        VpaConfig {
                            target_ref: VpaTargetRef::new(
                                VpaTargetRefApiVersion::AppsV1,
                                VpaTargetRefKind::Deployment,
                                "cert-manager".to_string(),
                            ),
                            container_policy: VpaContainerPolicy::new(
                                "*".to_string(),
                                Some(KubernetesCpuResourceUnit::MilliCpu(100)),
                                Some(KubernetesCpuResourceUnit::MilliCpu(500)),
                                Some(KubernetesMemoryResourceUnit::MebiByte(192)),
                                Some(KubernetesMemoryResourceUnit::GibiByte(3)),
                            ),
                        },
                        VpaConfig {
                            target_ref: VpaTargetRef::new(
                                VpaTargetRefApiVersion::AppsV1,
                                VpaTargetRefKind::Deployment,
                                "cert-manager-cainjector".to_string(),
                            ),
                            container_policy: VpaContainerPolicy::new(
                                "*".to_string(),
                                Some(KubernetesCpuResourceUnit::MilliCpu(100)),
                                Some(KubernetesCpuResourceUnit::MilliCpu(500)),
                                Some(KubernetesMemoryResourceUnit::MebiByte(192)),
                                Some(KubernetesMemoryResourceUnit::GibiByte(3)),
                            ),
                        },
                        VpaConfig {
                            target_ref: VpaTargetRef::new(
                                VpaTargetRefApiVersion::AppsV1,
                                VpaTargetRefKind::Deployment,
                                "cert-manager-webhook".to_string(),
                            ),
                            container_policy: VpaContainerPolicy::new(
                                "*".to_string(),
                                Some(KubernetesCpuResourceUnit::MilliCpu(50)),
                                Some(KubernetesCpuResourceUnit::MilliCpu(200)),
                                Some(KubernetesMemoryResourceUnit::MebiByte(128)),
                                Some(KubernetesMemoryResourceUnit::MebiByte(512)),
                            ),
                        },
                        VpaConfig {
                            target_ref: VpaTargetRef::new(
                                VpaTargetRefApiVersion::AppsV1,
                                VpaTargetRefKind::Deployment,
                                "qovery-cert-manager-webhook".to_string(),
                            ),
                            container_policy: VpaContainerPolicy::new(
                                "*".to_string(),
                                Some(KubernetesCpuResourceUnit::MilliCpu(50)),
                                Some(KubernetesCpuResourceUnit::MilliCpu(200)),
                                Some(KubernetesMemoryResourceUnit::MebiByte(64)),
                                Some(KubernetesMemoryResourceUnit::MebiByte(512)),
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
pub struct CertManagerChartChecker {}

impl CertManagerChartChecker {
    pub fn new() -> CertManagerChartChecker {
        CertManagerChartChecker {}
    }
}

impl Default for CertManagerChartChecker {
    fn default() -> Self {
        CertManagerChartChecker::new()
    }
}

impl ChartInstallationChecker for CertManagerChartChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO(ENG-1401): Implement chart install verification
        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::helm::{HelmChartNamespaces, UpdateStrategy};
    use crate::infrastructure::helm_charts::cert_manager_chart::CertManagerChart;
    use crate::infrastructure::helm_charts::{
        HelmChartResourcesConstraintType, HelmChartType, ToCommonHelmChart,
        get_helm_path_kubernetes_provider_sub_folder_name, get_helm_values_set_in_code_but_absent_in_values_file,
    };
    use crate::io_models::models::CustomerHelmChartsOverride;
    use std::env;
    use std::sync::Arc;

    fn get_cert_manager_chart_override() -> Arc<dyn Fn(String) -> Option<CustomerHelmChartsOverride>> {
        Arc::new(|_chart_name: String| -> Option<CustomerHelmChartsOverride> {
            Some(CustomerHelmChartsOverride {
                chart_name: CertManagerChart::chart_name(),
                chart_values: "".to_string(),
            })
        })
    }

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn cert_manager_chart_directory_exists_test() {
        // setup:
        let chart = CertManagerChart::new(
            None,
            false,
            HelmChartResourcesConstraintType::ChartDefault,
            HelmChartResourcesConstraintType::ChartDefault,
            HelmChartResourcesConstraintType::ChartDefault,
            UpdateStrategy::RollingUpdate,
            get_cert_manager_chart_override(),
            false,
            HelmChartNamespaces::CertManager,
            HelmChartNamespaces::KubeSystem,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared,),
            CertManagerChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn cert_manager_chart_values_file_exists_test() {
        // setup:
        let chart = CertManagerChart::new(
            None,
            false,
            HelmChartResourcesConstraintType::ChartDefault,
            HelmChartResourcesConstraintType::ChartDefault,
            HelmChartResourcesConstraintType::ChartDefault,
            UpdateStrategy::RollingUpdate,
            get_cert_manager_chart_override(),
            false,
            HelmChartNamespaces::CertManager,
            HelmChartNamespaces::KubeSystem,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_values_path = format!(
            "{}/lib/{}/bootstrap/chart_values/{}.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart.chart_values_path.helm_path(),
                HelmChartType::Shared,
            ),
            CertManagerChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
    }

    /// Make sure rust code deosn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn cert_manager_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = CertManagerChart::new(
            None,
            false,
            HelmChartResourcesConstraintType::ChartDefault,
            HelmChartResourcesConstraintType::ChartDefault,
            HelmChartResourcesConstraintType::ChartDefault,
            UpdateStrategy::RollingUpdate,
            get_cert_manager_chart_override(),
            false,
            HelmChartNamespaces::CertManager,
            HelmChartNamespaces::KubeSystem,
        );
        let common_chart = chart.to_common_helm_chart().unwrap();

        // execute:
        let missing_fields = get_helm_values_set_in_code_but_absent_in_values_file(
            common_chart,
            format!(
                "/lib/{}/bootstrap/chart_values/{}.yaml",
                get_helm_path_kubernetes_provider_sub_folder_name(
                    chart.chart_values_path.helm_path(),
                    HelmChartType::Shared,
                ),
                CertManagerChart::chart_name()
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

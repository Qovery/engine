use std::ops::Add;
use std::sync::Arc;

use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, CommonChartVpa, HelmAction, HelmChartError,
    HelmChartNamespaces, PriorityClass, VpaConfig, VpaContainerPolicy, VpaTargetRef, VpaTargetRefApiVersion,
    VpaTargetRefKind,
};
use crate::infrastructure::helm_charts::qovery_source_registry::QoverySourceRegistry;
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::infrastructure::models::cloud_provider::Kind;
use crate::io_models::models::{CustomerHelmChartsOverride, KubernetesCpuResourceUnit, KubernetesMemoryResourceUnit};
use kube::Client;

pub struct AlloyChart {
    chart_prefix_path: Option<String>,
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    loki_kube_dns_name: String,
    customer_helm_chart_override: Option<CustomerHelmChartsOverride>,
    customer_helm_chart_vpa_override: Option<CustomerHelmChartsOverride>,
    enable_vpa: bool,
    namespace: HelmChartNamespaces,
    priority_class: PriorityClass,
    additional_chart_values: Vec<HelmChartValuesFilePath>,
    cloud_provider_kind: Kind,
}

impl AlloyChart {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        chart_prefix_path: Option<&str>,
        chart_values_location: HelmChartDirectoryLocation,
        loki_kube_dns_name: String,
        customer_helm_chart_fn: Arc<dyn Fn(String) -> Option<CustomerHelmChartsOverride>>,
        enable_vpa: bool,
        namespace: HelmChartNamespaces,
        priority_class: PriorityClass,
        karpenter_enabled: bool,
        cloud_provider_kind: Kind,
    ) -> Self {
        let mut additional_chart_values = vec![];
        if karpenter_enabled {
            add_chart_value(&mut additional_chart_values, chart_prefix_path, "alloy_with_karpenter");
        }

        AlloyChart {
            chart_prefix_path: chart_prefix_path.map(|s| s.to_string()),
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                AlloyChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                chart_values_location,
                AlloyChart::chart_name(),
            ),
            loki_kube_dns_name,
            customer_helm_chart_override: customer_helm_chart_fn(Self::chart_name()),
            customer_helm_chart_vpa_override: customer_helm_chart_fn(Self::chart_name().add(".vpa")),
            enable_vpa,
            namespace,
            priority_class,
            additional_chart_values,
            cloud_provider_kind,
        }
    }

    pub fn chart_name() -> String {
        "alloy".to_string()
    }
}

fn add_chart_value(values: &mut Vec<HelmChartValuesFilePath>, chart_prefix_path: Option<&str>, name: &str) {
    values.push(HelmChartValuesFilePath::new(
        chart_prefix_path,
        HelmChartDirectoryLocation::CommonFolder,
        name.to_string(),
    ));
}

impl ToCommonHelmChart for AlloyChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        let values_files: Vec<String> = std::iter::once(&self.chart_values_path)
            .chain(self.additional_chart_values.iter())
            .map(ToString::to_string)
            .collect();

        let mut chart_info = ChartInfo {
            name: AlloyChart::chart_name(),
            // Fresh chart: no legacy installs to force-reinstall.
            reinstall_chart_if_installed_version_is_below_than: None,
            path: self.chart_path.to_string(),
            namespace: self.namespace.clone(),
            values_files,
            values: {
                let source_registry = QoverySourceRegistry::from(&self.cloud_provider_kind);
                vec![
                    ChartSetValue {
                        key: "image.registry".to_string(),
                        value: source_registry.host(),
                    },
                    ChartSetValue {
                        key: "image.repository".to_string(),
                        value: source_registry.image_path("pub-mirror-alloy"),
                    },
                    ChartSetValue {
                        key: "image.tag".to_string(),
                        value: "v1.17.0".to_string(),
                    },
                    // River config reads this with sys.env("LOKI_WRITE_URL").
                    // Index [1] matches the LOKI_WRITE_URL element in the values file's alloy.extraEnv list
                    // (index [0] is NODE_NAME via fieldRef). Same proven `[index].field` --set pattern promtail used.
                    ChartSetValue {
                        key: "alloy.extraEnv[1].value".to_string(),
                        value: format!("http://{}/loki/api/v1/push", self.loki_kube_dns_name),
                    },
                ]
            },
            yaml_files_content: match self.customer_helm_chart_override.clone() {
                Some(x) => vec![x.to_chart_values_generated()],
                None => vec![],
            },
            // Alloy is on every node; large clusters take time to roll out.
            timeout_in_seconds: 1800,
            ..Default::default()
        };

        // Set custom priority class if provided (alloy chart uses controller.priorityClassName).
        if let PriorityClass::Qovery(priority_class) = &self.priority_class {
            chart_info.values.push(ChartSetValue {
                key: "controller.priorityClassName".to_string(),
                value: priority_class.to_string(),
            });
        }

        Ok(CommonChart {
            chart_info,
            chart_installation_checker: Some(Box::new(AlloyChartChecker::new())),
            vertical_pod_autoscaler: match self.enable_vpa {
                true => Some(CommonChartVpa::new(
                    self.chart_prefix_path.clone().unwrap_or(".".to_string()),
                    vec![VpaConfig {
                        target_ref: VpaTargetRef::new(
                            VpaTargetRefApiVersion::AppsV1,
                            VpaTargetRefKind::DaemonSet,
                            "alloy".to_string(),
                        ),
                        container_policy: VpaContainerPolicy::new(
                            "*".to_string(),
                            Some(KubernetesCpuResourceUnit::MilliCpu(100)),
                            Some(KubernetesCpuResourceUnit::MilliCpu(500)),
                            Some(KubernetesMemoryResourceUnit::MebiByte(128)),
                            Some(KubernetesMemoryResourceUnit::GibiByte(3)),
                        ),
                        customer_helm_chart_override: self.customer_helm_chart_vpa_override.clone(),
                    }],
                )),
                false => None,
            },
            pre_execute_action: None,
        })
    }
}

#[derive(Clone)]
pub struct AlloyChartChecker {}

impl AlloyChartChecker {
    pub fn new() -> AlloyChartChecker {
        AlloyChartChecker {}
    }
}

impl Default for AlloyChartChecker {
    fn default() -> Self {
        AlloyChartChecker::new()
    }
}

impl ChartInstallationChecker for AlloyChartChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO(ENG-1370): Implement chart install verification
        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

/// Transitional uninstaller for the deprecated promtail release (QOV-1949).
///
/// The migration to Alloy removed the promtail chart from the deploy set, but the
/// deploy loop only uninstalls a release when a chart with `HelmAction::Destroy` is
/// present. Without this, the old promtail release keeps running next to Alloy and
/// double-ships logs to Loki. `helm uninstall` runs with `--ignore-not-found`, so this
/// is a no-op on clusters that never had promtail. Remove this helper and its call
/// sites once all clusters have reconciled (a few releases).
pub fn promtail_uninstall_chart(namespace: HelmChartNamespaces) -> CommonChart {
    CommonChart {
        chart_info: ChartInfo {
            name: "promtail".to_string(),
            namespace,
            action: HelmAction::Destroy,
            ..Default::default()
        },
        chart_installation_checker: None,
        vertical_pod_autoscaler: None,
        pre_execute_action: None,
    }
}

#[cfg(test)]
mod tests {
    use crate::helm::{HelmChartNamespaces, PriorityClass};
    use crate::infrastructure::helm_charts::alloy_chart::AlloyChart;
    use crate::infrastructure::helm_charts::{
        HelmChartDirectoryLocation, HelmChartType, get_helm_path_kubernetes_provider_sub_folder_name,
    };
    use crate::infrastructure::models::cloud_provider::Kind;
    use crate::infrastructure::models::kubernetes::Kind as KubernetesKind;
    use crate::io_models::models::CustomerHelmChartsOverride;
    use std::env;
    use std::sync::Arc;

    fn get_alloy_chart_override() -> Arc<dyn Fn(String) -> Option<CustomerHelmChartsOverride>> {
        Arc::new(|_chart_name: String| -> Option<CustomerHelmChartsOverride> {
            Some(CustomerHelmChartsOverride {
                chart_name: AlloyChart::chart_name(),
                chart_values: "".to_string(),
            })
        })
    }

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn alloy_chart_directory_exists_test() {
        let chart = AlloyChart::new(
            None,
            HelmChartDirectoryLocation::CloudProviderFolder,
            "whatever".to_string(),
            get_alloy_chart_override(),
            false,
            HelmChartNamespaces::KubeSystem,
            PriorityClass::Default,
            false,
            Kind::Aws,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared),
            AlloyChart::chart_name(),
        );

        let values_file = std::fs::File::open(&chart_path);
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn alloy_chart_values_file_exists_test() {
        let chart = AlloyChart::new(
            None,
            HelmChartDirectoryLocation::CloudProviderFolder,
            "whatever".to_string(),
            get_alloy_chart_override(),
            false,
            HelmChartNamespaces::KubeSystem,
            PriorityClass::Default,
            false,
            Kind::Aws,
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_values_path = format!(
            "{}/lib/{}/bootstrap/chart_values/{}.j2.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart.chart_values_path.helm_path(),
                HelmChartType::CloudProviderSpecific(KubernetesKind::Eks)
            ),
            AlloyChart::chart_name(),
        );

        let values_file = std::fs::File::open(&chart_values_path);
        assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
    }
}

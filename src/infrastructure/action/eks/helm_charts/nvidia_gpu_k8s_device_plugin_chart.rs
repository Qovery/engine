use crate::{
    helm::{ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmAction, HelmChartError},
    infrastructure::helm_charts::{
        HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
    },
};
// TODO(benjaminch): Handle deinstallation in case there are no GPU node pools

pub struct NvidiaGpuK8sDevicePluginChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
}

impl NvidiaGpuK8sDevicePluginChart {
    pub fn new(chart_prefix_path: Option<&str>) -> Self {
        let chart_path = HelmChartPath::new(
            chart_prefix_path,
            HelmChartDirectoryLocation::CloudProviderFolder,
            Self::chart_name(),
        );

        Self {
            chart_path,
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                NvidiaGpuK8sDevicePluginChart::chart_name(),
            ),
        }
    }

    pub fn chart_name() -> String {
        "nvidia-device-plugin".to_string()
    }
}

impl ToCommonHelmChart for NvidiaGpuK8sDevicePluginChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        Ok(CommonChart {
            chart_info: ChartInfo {
                name: NvidiaGpuK8sDevicePluginChart::chart_name(),
                action: HelmAction::Deploy,
                path: self.chart_path.to_string(),
                values_files: vec![self.chart_values_path.to_string()],
                values: vec![
                    ChartSetValue {
                        key: "nameOverride".to_string(),
                        value: NvidiaGpuK8sDevicePluginChart::chart_name(),
                    },
                    ChartSetValue {
                        key: "fullnameOverride".to_string(),
                        value: NvidiaGpuK8sDevicePluginChart::chart_name(),
                    },
                ],
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(NvidiaGpuK8sDevicePluginChartChecker::new())),
            vertical_pod_autoscaler: None,
            pre_execute_action: None,
        })
    }
}

#[derive(Clone)]
pub struct NvidiaGpuK8sDevicePluginChartChecker {}

impl NvidiaGpuK8sDevicePluginChartChecker {
    pub fn new() -> NvidiaGpuK8sDevicePluginChartChecker {
        NvidiaGpuK8sDevicePluginChartChecker {}
    }
}

impl ChartInstallationChecker for NvidiaGpuK8sDevicePluginChartChecker {
    fn verify_installation(&self, _kube_client: &kube::Client) -> Result<(), crate::errors::CommandError> {
        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::helm_charts::{
        HelmChartType, get_helm_path_kubernetes_provider_sub_folder_name,
        get_helm_values_set_in_code_but_absent_in_values_file,
    };
    use crate::infrastructure::models::kubernetes::Kind as KubernetesKind;
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn nvidia_gpu_k8s_device_plugin_chart_directory_exists_test() {
        // setup:
        let chart = NvidiaGpuK8sDevicePluginChart::new(None);
        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart.chart_path.helm_path(),
                HelmChartType::CloudProviderSpecific(KubernetesKind::Eks),
            ),
            NvidiaGpuK8sDevicePluginChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn nvidia_gpu_k8s_device_plugin_chart_values_file_exists_test() {
        // setup:
        let chart = NvidiaGpuK8sDevicePluginChart::new(None);

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_values_path = format!(
            "{}/lib/{}/bootstrap/chart_values/{}.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(
                chart.chart_values_path.helm_path(),
                HelmChartType::CloudProviderSpecific(KubernetesKind::Eks),
            ),
            NvidiaGpuK8sDevicePluginChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{chart_values_path}`");
    }

    /// Make sure rust code deosn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn nvidia_gpu_k8s_device_plugin_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = NvidiaGpuK8sDevicePluginChart::new(None);
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
                NvidiaGpuK8sDevicePluginChart::chart_name(),
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

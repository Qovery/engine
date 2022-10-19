use crate::cloud_provider::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmChartNamespaces,
};
use crate::cloud_provider::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::cmd::helm_utils::CRDSUpdate;
use crate::errors::CommandError;
use kube::Client;

pub type StorageClassName = String;

pub struct KubePrometheusStackChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    storage_class_name: StorageClassName,
    prometheus_internal_url: String,
    prometheus_namespace: HelmChartNamespaces,
    kubelet_service_monitor_resource_enabled: bool,
}

impl KubePrometheusStackChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        storage_class_name: StorageClassName,
        prometheus_internal_url: String,
        prometheus_namespace: HelmChartNamespaces,
        kubelet_service_monitor_resource_enabled: bool,
    ) -> Self {
        KubePrometheusStackChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                KubePrometheusStackChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                KubePrometheusStackChart::chart_name(),
            ),
            storage_class_name,
            prometheus_internal_url,
            prometheus_namespace,
            kubelet_service_monitor_resource_enabled,
        }
    }

    fn chart_name() -> String {
        "kube-prometheus-stack".to_string()
    }
}

impl ToCommonHelmChart for KubePrometheusStackChart {
    fn to_common_helm_chart(&self) -> CommonChart {
        CommonChart {
            chart_info: ChartInfo {
                name: KubePrometheusStackChart::chart_name(),
                path: self.chart_path.to_string(),
                namespace: self.prometheus_namespace,
                // high timeout because on bootstrap, it's one of the biggest dependencies and on upgrade, it can takes time
                // to upgrade because of the CRD and the number of elements it has to deploy
                timeout_in_seconds: 480,
                crds_update: Some(CRDSUpdate{
                    path:"https://raw.githubusercontent.com/prometheus-operator/prometheus-operator/v0.56.0/example/prometheus-operator-crd".to_string(),
                    resources: vec![
                        "monitoring.coreos.com_alertmanagerconfigs.yaml".to_string(),
                        "monitoring.coreos.com_alertmanagers.yaml".to_string(),
                        "monitoring.coreos.com_podmonitors.yaml".to_string(),
                        "monitoring.coreos.com_probes.yaml".to_string(),
                        "monitoring.coreos.com_prometheuses.yaml".to_string(),
                        "monitoring.coreos.com_prometheusrules.yaml".to_string(),
                        "monitoring.coreos.com_servicemonitors.yaml".to_string(),
                        "monitoring.coreos.com_thanosrulers.yaml".to_string(),
                    ]
                }),
                values_files: vec![self.chart_values_path.to_string()],
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
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(KubePrometheusStackChartChecker::new())),
        }
    }
}

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
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::helm::HelmChartNamespaces;
    use crate::cloud_provider::helm_charts::kube_prometheus_stack_chart::KubePrometheusStackChart;
    use crate::cloud_provider::helm_charts::{
        get_helm_values_set_in_code_but_absent_in_values_file, ToCommonHelmChart,
    };
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn kube_prometheus_stack_chart_directory_exists_test() {
        // setup:
        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/common/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            KubePrometheusStackChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{}`", chart_path);
    }

    /// Makes sure chart values file exists.
    #[test]
    fn kube_prometheus_stack_chart_values_file_exists_test() {
        // setup:
        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_values_path = format!(
            "{}/lib/common/bootstrap/chart_values/{}.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            KubePrometheusStackChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_values_path);

        // verify:
        assert!(values_file.is_ok(), "Chart values file should exist: `{}`", chart_values_path);
    }

    /// Make sure rust code deosn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = KubePrometheusStackChart::new(
            None,
            "whatever".to_string(),
            "whatever".to_string(),
            HelmChartNamespaces::Prometheus,
            true,
        )
        .to_common_helm_chart();

        // execute:
        let missing_fields = get_helm_values_set_in_code_but_absent_in_values_file(
            chart,
            "/lib/common/bootstrap/chart_values/kube-prometheus-stack.yaml".to_string(),
        );

        // verify:
        assert!(missing_fields.is_none(), "Some fields are missing in values file, add those (make sure they still exist in chart values), fields: {}", missing_fields.unwrap_or_default().join(","));
    }
}

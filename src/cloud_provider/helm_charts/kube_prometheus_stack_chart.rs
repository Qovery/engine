use crate::cloud_provider::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmChartNamespaces,
};
use crate::cloud_provider::helm_charts::{HelmChartDirectoryLocation, HelmChartPath, ToCommonHelmChart};
use crate::cmd::helm_utils::CRDSUpdate;
use crate::errors::CommandError;
use kube::Client;

pub type StorageClassName = String;

pub struct KubePrometheusStackChart {
    chart_path: HelmChartPath,
    storage_class_name: StorageClassName,
    prometheus_internal_url: String,
    prometheus_namespace: HelmChartNamespaces,
}

impl KubePrometheusStackChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        storage_class_name: StorageClassName,
        prometheus_internal_url: String,
        prometheus_namespace: HelmChartNamespaces,
    ) -> Self {
        KubePrometheusStackChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CloudProviderFolder,
                KubePrometheusStackChart::chart_name(),
            ),
            storage_class_name,
            prometheus_internal_url,
            prometheus_namespace,
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
                values: vec![
                    ChartSetValue {
                        key: "installCRDs".to_string(),
                        value: "true".to_string(),
                    },
                    ChartSetValue {
                        key: "nameOverride".to_string(),
                        value: "prometheus-operator".to_string(),
                    },
                    ChartSetValue {
                        key: "fullnameOverride".to_string(),
                        value: "prometheus-operator".to_string(),
                    },
                    ChartSetValue {
                        key: "prometheusOperator.tls.enabled".to_string(),
                        value: "false".to_string(),
                    },
                    ChartSetValue {
                        key: "prometheusOperator.admissionWebhooks.enabled".to_string(),
                        value: "false".to_string(),
                    },
                    ChartSetValue {
                        key: "prometheus-node-exporter.prometheus.monitor.enabled".to_string(),
                        value: "false".to_string(),
                    },
                    ChartSetValue {
                        key: "kubelet.serviceMonitor.resource".to_string(),
                        value: "true".to_string(),
                    },
                    ChartSetValue {
                        key: "kubelet.serviceMonitor.resourcePath".to_string(),
                        value: "/metrics/resource".to_string(),
                    },
                    // Default rules
                    ChartSetValue {
                        key: "defaultRules.create".to_string(),
                        value: "true".to_string(),
                    },
                    ChartSetValue {
                        key: "defaultRules.rules.alertmanager".to_string(),
                        value: "false".to_string(),
                    },
                    ChartSetValue {
                        key: "defaultRules.rules.etcd".to_string(),
                        value: "false".to_string(),
                    },
                    ChartSetValue {
                        key: "defaultRules.rules.configReloaders".to_string(),
                        value: "true".to_string(),
                    },
                    ChartSetValue {
                        key: "defaultRules.rules.kubeApiserverAvailability".to_string(),
                        value: "false".to_string(),
                    },
                    ChartSetValue {
                        key: "defaultRules.rules.kubeApiserverBurnrate".to_string(),
                        value: "false".to_string(),
                    },
                    ChartSetValue {
                        key: "defaultRules.rules.kubeProxy".to_string(),
                        value: "false".to_string(),
                    },
                    ChartSetValue {
                        key: "defaultRules.rules.kubeApiserverHistogram".to_string(),
                        value: "false".to_string(),
                    },
                    ChartSetValue {
                        key: "defaultRules.rules.kubeApiserverSlos".to_string(),
                        value: "false".to_string(),
                    },
                    ChartSetValue {
                        key: "defaultRules.rules.kubeStateMetrics".to_string(),
                        value: "false".to_string(),
                    },
                    ChartSetValue {
                        key: "defaultRules.rules.nodeExporterAlerting".to_string(),
                        value: "false".to_string(),
                    },
                    ChartSetValue {
                        key: "defaultRules.rules.nodeExporterRecording".to_string(),
                        value: "false".to_string(),
                    },
                    // Prometheus
                    ChartSetValue {
                        key: "prometheus.enabled".to_string(),
                        value: "true".to_string(),
                    },
                    ChartSetValue {
                        key: "prometheus.prometheusSpec.externalUrl".to_string(),
                        value: self.prometheus_internal_url.to_string(),
                    },
                    ChartSetValue {
                        key: "prometheus.prometheusSpec.serviceMonitorSelectorNilUsesHelmValues".to_string(),
                        value: "false".to_string(),
                    },
                    ChartSetValue {
                        key: "prometheus.prometheusSpec.retention".to_string(),
                        value: "90d".to_string(),
                    },
                    ChartSetValue {
                        key: "prometheus.prometheusSpec.retentionSize".to_string(),
                        value: "40GB".to_string(),
                    },
                    ChartSetValue {
                        key: "prometheus.prometheusSpec.walCompression".to_string(),
                        value: "true".to_string(),
                    },
                    ChartSetValue {
                        key: "prometheus.prometheusSpec.storageSpec.volumeClaimTemplate.spec.storageClassName".to_string(),
                        value: self.storage_class_name.to_string(),
                    },
                    ChartSetValue {
                        key: "prometheus.prometheusSpec.storageSpec.volumeClaimTemplate.spec.accessModes".to_string(),
                        value: "{ReadWriteOnce}".to_string(),
                    },
                    ChartSetValue {
                        key: "prometheus.prometheusSpec.storageSpec.volumeClaimTemplate.spec.resources.requests.storage".to_string(),
                        value: "50Gi".to_string(),
                    },
                    // Alert manager
                    ChartSetValue {
                        key: "alertmanager.enabled".to_string(),
                        value: "false".to_string(),
                    },
                    // Grafana
                    ChartSetValue {
                        key: "grafana.enabled".to_string(),
                        value: "false".to_string(),
                    },
                    ChartSetValue {
                        key: "grafana.serviceMonitor.enabled".to_string(),
                        value: "false".to_string(),
                    },
                    // Kube Controller Manager
                    ChartSetValue {
                        key: "kubeControllerManager.enabled".to_string(),
                        value: "false".to_string(),
                    },
                    // Kube Etcd
                    ChartSetValue {
                        key: "kubeEtcd.enabled".to_string(),
                        value: "false".to_string(),
                    },
                    // Kube Scheduler
                    ChartSetValue {
                        key: "kubeScheduler.enabled".to_string(),
                        value: "false".to_string(),
                    },
                    // Kube Proxy
                    ChartSetValue {
                        key: "kubeProxy.enabled".to_string(),
                        value: "false".to_string(),
                    },
                    // Kube State Metrics
                    ChartSetValue {
                        key: "kubeStateMetrics.enabled".to_string(),
                        value: "false".to_string(),
                    },
                    // Limits prometheus-node-exporter
                    ChartSetValue {
                        key: "prometheus-node-exporter.resources.limits.cpu".to_string(),
                        value: "20m".to_string(),
                    },
                    ChartSetValue {
                        key: "prometheus-node-exporter.resources.requests.cpu".to_string(),
                        value: "10m".to_string(),
                    },
                    ChartSetValue {
                        key: "prometheus-node-exporter.resources.limits.memory".to_string(),
                        value: "32Mi".to_string(),
                    },
                    ChartSetValue {
                        key: "prometheus-node-exporter.resources.requests.memory".to_string(),
                        value: "32Mi".to_string(),
                    },
                    // resources limits
                    ChartSetValue {
                        key: "prometheusOperator.resources.limits.cpu".to_string(),
                        value: "1".to_string(),
                    },
                    ChartSetValue {
                        key: "prometheusOperator.resources.requests.cpu".to_string(),
                        value: "500m".to_string(),
                    },
                    ChartSetValue {
                        key: "prometheusOperator.resources.limits.memory".to_string(),
                        value: "1Gi".to_string(),
                    },
                    ChartSetValue {
                        key: "prometheusOperator.resources.requests.memory".to_string(),
                        value: "1Gi".to_string(),
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

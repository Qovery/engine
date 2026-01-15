use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmAction, HelmChartError, HelmChartNamespaces,
};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartResources, HelmChartValuesFilePath, ToCommonHelmChart,
    ToHelmChartValue,
};
use crate::infrastructure::models::kubernetes::keda::{KedaAvailability, KedaResourceProfile};
use kube::Client;

pub struct KedaChart {
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    enable_monitoring: bool,
    action: HelmAction,
    resource_profile: KedaResourceProfile,
    availability: KedaAvailability,
    keda_operator_role_arn: Option<String>,
    keda_metrics_server_role_arn: Option<String>,
}

impl KedaChart {
    pub fn new(
        chart_prefix_path: Option<&str>,
        enable_monitoring: bool,
        action: HelmAction,
        resource_profile: KedaResourceProfile,
        availability: KedaAvailability,
        keda_operator_role_arn: Option<String>,
        keda_metrics_server_role_arn: Option<String>,
    ) -> Self {
        KedaChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                KedaChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                KedaChart::chart_name(),
            ),
            enable_monitoring,
            action,
            resource_profile,
            availability,
            keda_operator_role_arn,
            keda_metrics_server_role_arn,
        }
    }

    pub fn chart_name() -> String {
        "keda".to_string()
    }

    fn operator_resources(&self) -> HelmChartResources {
        match self.resource_profile {
            KedaResourceProfile::Low => HelmChartResources::new("50m", "500m", "64Mi", "512Mi"),
            KedaResourceProfile::Normal => HelmChartResources::new("100m", "1", "100Mi", "1000Mi"),
            KedaResourceProfile::High => HelmChartResources::new("200m", "2", "256Mi", "2Gi"),
        }
    }

    fn metrics_server_resources(&self) -> HelmChartResources {
        match self.resource_profile {
            KedaResourceProfile::Low => HelmChartResources::new("50m", "500m", "64Mi", "512Mi"),
            KedaResourceProfile::Normal => HelmChartResources::new("100m", "1", "100Mi", "1000Mi"),
            KedaResourceProfile::High => HelmChartResources::new("200m", "2", "256Mi", "2Gi"),
        }
    }

    fn webhooks_resources(&self) -> HelmChartResources {
        match self.resource_profile {
            KedaResourceProfile::Low => HelmChartResources::new("10m", "50m", "25Mi", "100Mi"),
            KedaResourceProfile::Normal => HelmChartResources::new("25m", "100m", "50Mi", "200Mi"),
            KedaResourceProfile::High => HelmChartResources::new("50m", "200m", "100Mi", "256Mi"),
        }
    }

    fn replicas(&self) -> (u16, u16, u16) {
        match self.availability {
            KedaAvailability::Normal => (1, 1, 1),
            KedaAvailability::High => (2, 2, 2),
        }
    }
}

impl ToCommonHelmChart for KedaChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        let (operator_replicas, metrics_server_replicas, webhooks_replicas) = self.replicas();
        let operator_resources = self.operator_resources();
        let metrics_server_resources = self.metrics_server_resources();
        let webhooks_resources = self.webhooks_resources();

        let mut values = vec![
            ChartSetValue {
                key: "operator.replicaCount".to_string(),
                value: operator_replicas.to_string(),
            },
            ChartSetValue {
                key: "resources.operator.requests.cpu".to_string(),
                value: operator_resources.request_cpu.to_helm_chart_value(),
            },
            ChartSetValue {
                key: "resources.operator.requests.memory".to_string(),
                value: operator_resources.request_memory.to_helm_chart_value(),
            },
            ChartSetValue {
                key: "resources.operator.limits.cpu".to_string(),
                value: operator_resources.limit_cpu.to_helm_chart_value(),
            },
            ChartSetValue {
                key: "resources.operator.limits.memory".to_string(),
                value: operator_resources.limit_memory.to_helm_chart_value(),
            },
            ChartSetValue {
                key: "metricsServer.replicaCount".to_string(),
                value: metrics_server_replicas.to_string(),
            },
            ChartSetValue {
                key: "resources.metricServer.requests.cpu".to_string(),
                value: metrics_server_resources.request_cpu.to_helm_chart_value(),
            },
            ChartSetValue {
                key: "resources.metricServer.requests.memory".to_string(),
                value: metrics_server_resources.request_memory.to_helm_chart_value(),
            },
            ChartSetValue {
                key: "resources.metricServer.limits.cpu".to_string(),
                value: metrics_server_resources.limit_cpu.to_helm_chart_value(),
            },
            ChartSetValue {
                key: "resources.metricServer.limits.memory".to_string(),
                value: metrics_server_resources.limit_memory.to_helm_chart_value(),
            },
            ChartSetValue {
                key: "webhooks.replicaCount".to_string(),
                value: webhooks_replicas.to_string(),
            },
            ChartSetValue {
                key: "resources.webhooks.requests.cpu".to_string(),
                value: webhooks_resources.request_cpu.to_helm_chart_value(),
            },
            ChartSetValue {
                key: "resources.webhooks.requests.memory".to_string(),
                value: webhooks_resources.request_memory.to_helm_chart_value(),
            },
            ChartSetValue {
                key: "resources.webhooks.limits.cpu".to_string(),
                value: webhooks_resources.limit_cpu.to_helm_chart_value(),
            },
            ChartSetValue {
                key: "resources.webhooks.limits.memory".to_string(),
                value: webhooks_resources.limit_memory.to_helm_chart_value(),
            },
            ChartSetValue {
                key: "prometheus.operator.enabled".to_string(),
                value: self.enable_monitoring.to_string(),
            },
            ChartSetValue {
                key: "prometheus.operator.serviceMonitor.enabled".to_string(),
                value: self.enable_monitoring.to_string(),
            },
            ChartSetValue {
                key: "prometheus.operator.prometheusRule.enabled".to_string(),
                value: self.enable_monitoring.to_string(),
            },
            ChartSetValue {
                key: "prometheus.metricServer.enabled".to_string(),
                value: self.enable_monitoring.to_string(),
            },
            ChartSetValue {
                key: "prometheus.metricServer.serviceMonitor.enabled".to_string(),
                value: self.enable_monitoring.to_string(),
            },
        ];

        // Add KEDA operator role ARN if provided
        if let Some(operator_role_arn) = &self.keda_operator_role_arn {
            values.push(ChartSetValue {
                key: r"serviceAccount.annotations.eks\.amazonaws\.com/role-arn".to_string(),
                value: operator_role_arn.clone(),
            });
        }

        // Add KEDA metrics server role ARN if provided
        if let Some(metrics_server_role_arn) = &self.keda_metrics_server_role_arn {
            values.push(ChartSetValue {
                key: r"metricsServer.serviceAccount.annotations.eks\.amazonaws\.com/role-arn".to_string(),
                value: metrics_server_role_arn.clone(),
            });
        }

        Ok(CommonChart {
            chart_info: ChartInfo {
                name: KedaChart::chart_name(),
                action: self.action.clone(),
                namespace: HelmChartNamespaces::KubeSystem,
                path: self.chart_path.to_string(),
                values_files: vec![self.chart_values_path.to_string()],
                values,
                ..Default::default()
            },
            chart_installation_checker: match self.action {
                HelmAction::Deploy => Some(Box::new(KedaChartChecker::new())),
                HelmAction::Destroy => None,
            },
            vertical_pod_autoscaler: None,
        })
    }
}

#[derive(Clone)]
pub struct KedaChartChecker {}

impl KedaChartChecker {
    pub fn new() -> Self {
        KedaChartChecker {}
    }
}

impl Default for KedaChartChecker {
    fn default() -> Self {
        KedaChartChecker::new()
    }
}

impl ChartInstallationChecker for KedaChartChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO: Implement verification
        // Could check for keda-operator deployment in kube-system namespace
        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_keda_chart_applies_resource_and_ha_overrides() {
        let chart = KedaChart::new(
            None,
            false,
            HelmAction::Deploy,
            KedaResourceProfile::High,
            KedaAvailability::High,
            None,
            None,
        );

        let common = chart.to_common_helm_chart().expect("chart generation should succeed");
        let values: HashMap<_, _> = common
            .chart_info
            .values
            .iter()
            .map(|v| (v.key.clone(), v.value.clone()))
            .collect();

        assert_eq!(values.get("operator.replicaCount").unwrap(), "2");
        assert_eq!(values.get("metricsServer.replicaCount").unwrap(), "2");
        assert_eq!(values.get("webhooks.replicaCount").unwrap(), "2");
        assert_eq!(values.get("resources.operator.requests.cpu").unwrap(), "200m");
        assert_eq!(values.get("resources.metricServer.limits.memory").unwrap(), "2Gi");
        assert_eq!(values.get("resources.webhooks.requests.cpu").unwrap(), "50m");
    }
}

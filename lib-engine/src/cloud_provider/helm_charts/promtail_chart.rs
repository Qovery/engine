use crate::cloud_provider::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmChartNamespaces,
};
use crate::cloud_provider::helm_charts::{HelmChartDirectoryLocation, HelmChartPath, ToCommonHelmChart};
use crate::errors::CommandError;
use kube::Client;
use semver::Version;

pub struct PromtailChart {
    chart_path: HelmChartPath,
    loki_kube_dns_name: String,
}

impl PromtailChart {
    pub fn new(chart_prefix_path: Option<&str>, loki_kube_dns_name: String) -> Self {
        PromtailChart {
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                PromtailChart::chart_name(),
            ),
            loki_kube_dns_name,
        }
    }

    fn chart_name() -> String {
        "promtail".to_string()
    }
}

impl ToCommonHelmChart for PromtailChart {
    fn to_common_helm_chart(&self) -> CommonChart {
        CommonChart {
            chart_info: ChartInfo {
                name: PromtailChart::chart_name(),
                last_breaking_version_requiring_restart: Some(Version::new(5, 1, 0)),
                path: self.chart_path.to_string(),
                // because of priorityClassName, we need to add it to kube-system
                namespace: HelmChartNamespaces::KubeSystem,
                values: vec![
                    ChartSetValue {
                        key: "config.clients[0].url".to_string(),
                        value: format!("http://{}/loki/api/v1/push", self.loki_kube_dns_name),
                    },
                    // it's mandatory to get this class to ensure paused infra will behave properly on restore
                    ChartSetValue {
                        key: "priorityClassName".to_string(),
                        value: "system-node-critical".to_string(),
                    },
                    ChartSetValue {
                        key: "config.snippets.extraRelabelConfigs[0].action".to_string(),
                        value: "labelmap".to_string(),
                    },
                    ChartSetValue {
                        key: "config.snippets.extraRelabelConfigs[0].regex".to_string(), // # We need this config in order for the cluster agent to retrieve the log of the service
                        value: "__meta_kubernetes_pod_label_(appId|qovery_com_service_id|qovery_com_service_type|qovery_com_environment_id)".to_string(),
                    },
                    // resources limits
                    ChartSetValue {
                        key: "resources.limits.cpu".to_string(),
                        value: "100m".to_string(),
                    },
                    ChartSetValue {
                        key: "resources.requests.cpu".to_string(),
                        value: "100m".to_string(),
                    },
                    ChartSetValue {
                        key: "resources.limits.memory".to_string(),
                        value: "128Mi".to_string(),
                    },
                    ChartSetValue {
                        key: "resources.requests.memory".to_string(),
                        value: "128Mi".to_string(),
                    },
                ],
                ..Default::default()
            },
            chart_installation_checker: Some(Box::new(PromtailChartChecker::new())),
        }
    }
}

pub struct PromtailChartChecker {}

impl PromtailChartChecker {
    pub fn new() -> PromtailChartChecker {
        PromtailChartChecker {}
    }
}

impl Default for PromtailChartChecker {
    fn default() -> Self {
        PromtailChartChecker::new()
    }
}

impl ChartInstallationChecker for PromtailChartChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        // TODO(ENG-1370): Implement chart install verification
        Ok(())
    }
}

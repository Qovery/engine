use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmAction, HelmChartError, HelmChartNamespaces,
};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::io_models::metrics::{AlertConfigAlert, AlertConfigReceiver, AlertManagerConfig};
use crate::utilities::to_short_id;
use kube::Client;

pub struct AlertConfigChart {
    action: HelmAction,
    alert_config: Option<AlertManagerConfig>,
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    prometheus_namespace: HelmChartNamespaces,
    _cluster_name: String,
}

impl AlertConfigChart {
    pub fn new(
        action: HelmAction,
        prometheus_namespace: HelmChartNamespaces,
        chart_prefix_path: Option<&str>,
        cluster_name: &str,
        alert_config: Option<AlertManagerConfig>,
    ) -> Self {
        AlertConfigChart {
            action,
            alert_config,
            chart_path: HelmChartPath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                AlertConfigChart::chart_name(),
            ),
            chart_values_path: HelmChartValuesFilePath::new(
                chart_prefix_path,
                HelmChartDirectoryLocation::CommonFolder,
                AlertConfigChart::chart_name(),
            ),
            prometheus_namespace,
            _cluster_name: cluster_name.to_string(),
        }
    }

    pub fn chart_name() -> String {
        "qovery-alert-config".to_string()
    }
}

impl ToCommonHelmChart for AlertConfigChart {
    fn to_common_helm_chart(&self) -> Result<CommonChart, HelmChartError> {
        let values_files = vec![self.chart_values_path.to_string()];
        let mut values = vec![];
        let mut values_string = vec![];
        let mut action = self.action.clone();

        if let Some(alert_config) = &self.alert_config
            && alert_config.enabled
        {
            if let Some(config_name) = &alert_config.config_name {
                values.push(ChartSetValue {
                    key: "alertManagerConfigName".to_string(),
                    value: config_name.clone(),
                })
            }

            for (index, receiver) in alert_config.receivers.iter().enumerate() {
                values.extend(build_receiver_values(index, receiver));
            }
            for (index, alert) in alert_config.alerts.iter().enumerate() {
                let alert_values = build_alert_values(index, alert);

                values.extend(alert_values.values);
                values_string.extend(alert_values.values_string);
            }
        } else {
            action = HelmAction::Destroy;
        }

        Ok(CommonChart {
            chart_info: ChartInfo {
                action,
                name: AlertConfigChart::chart_name(),
                path: self.chart_path.to_string(),
                reinstall_chart_if_installed_version_is_below_than: None,
                namespace: self.prometheus_namespace.clone(),
                values_files,
                values,
                values_string,
                yaml_files_content: vec![],
                ..Default::default()
            },
            chart_installation_checker: None,
            vertical_pod_autoscaler: None,
        })
    }
}

fn build_receiver_values(index: usize, receiver: &AlertConfigReceiver) -> Vec<ChartSetValue> {
    match receiver {
        AlertConfigReceiver::SlackConfig {
            long_id,
            name,
            api_url,
            channel,
        } => vec![
            ChartSetValue {
                key: format!("receivers[{index}].id"),
                value: format!("qovery-alert-receiver-{}", to_short_id(long_id)),
            },
            ChartSetValue {
                key: format!("receivers[{index}].name"),
                value: name.clone(),
            },
            ChartSetValue {
                key: format!("receivers[{index}].slack.channel"),
                value: channel.clone(),
            },
            ChartSetValue {
                key: format!("receivers[{index}].slack.webhookURL"),
                value: api_url.clone(),
            },
        ],
    }
}

#[derive(Debug, Clone)]
struct AlertChartValues {
    values: Vec<ChartSetValue>,
    values_string: Vec<ChartSetValue>,
}

fn build_alert_values(index: usize, alert: &AlertConfigAlert) -> AlertChartValues {
    let mut values = Vec::with_capacity(5);
    let mut values_string = Vec::with_capacity(alert.labels.len());

    values.extend([
        ChartSetValue {
            key: format!("alerts[{index}].long_id"),
            value: format!("qovery-alert-{}", to_short_id(&alert.long_id)),
        },
        ChartSetValue {
            key: format!("alerts[{index}].name"),
            value: alert.name.clone(),
        },
        ChartSetValue {
            key: format!("alerts[{index}].expr"),
            value: alert.expr.clone(),
        },
        ChartSetValue {
            key: format!("alerts[{index}].for"),
            value: alert.r#for.clone(),
        },
    ]);

    if let Some(summary) = &alert.summary {
        values.push(ChartSetValue {
            key: format!("alerts[{index}].annotations.summary"),
            value: summary.clone(),
        })
    }

    if let Some(description) = &alert.description {
        values.push(ChartSetValue {
            key: format!("alerts[{index}].annotations.description"),
            value: description.clone(),
        })
    }

    if let Some(runbook_url) = &alert.runbook_url {
        values.push(ChartSetValue {
            key: format!("alerts[{index}].annotations.runbook_url"),
            value: runbook_url.clone(),
        })
    }

    values_string.extend(alert.labels.iter().map(|(key, value)| ChartSetValue {
        key: format!("alerts[{index}].labels.{key}"),
        value: value.clone(),
    }));

    AlertChartValues { values, values_string }
}

#[derive(Clone)]
pub struct AlertConfigChartChecker {}

impl AlertConfigChartChecker {
    pub fn new() -> AlertConfigChartChecker {
        AlertConfigChartChecker {}
    }
}

impl Default for AlertConfigChartChecker {
    fn default() -> Self {
        AlertConfigChartChecker::new()
    }
}

impl ChartInstallationChecker for AlertConfigChartChecker {
    fn verify_installation(&self, _kube_client: &Client) -> Result<(), CommandError> {
        Ok(())
    }

    fn clone_dyn(&self) -> Box<dyn ChartInstallationChecker> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use crate::helm::{HelmAction, HelmChartNamespaces};

    use crate::infrastructure::helm_charts::alert_config_chart::AlertConfigChart;
    use crate::infrastructure::helm_charts::{
        HelmChartType, ToCommonHelmChart, get_helm_path_kubernetes_provider_sub_folder_name,
        get_helm_values_set_in_code_but_absent_in_values_file,
    };
    use crate::infrastructure::models::kubernetes::Kind;
    use crate::io_models::metrics::AlertManagerConfig;
    use std::env;

    /// Makes sure chart directory containing all YAML files exists.
    #[test]
    fn alert_config_chart_directory_exists_test() {
        // setup:
        let chart = AlertConfigChart::new(
            HelmAction::Deploy,
            HelmChartNamespaces::Prometheus,
            None,
            "cluster-name",
            Some(AlertManagerConfig {
                enabled: true,
                default_rule_labels: None,
                spec_config_secret: None,
                spec_external_url: None,
                receivers: vec![],
                alerts: vec![],
                config_name: None,
            }),
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        let chart_path = format!(
            "{}/lib/{}/bootstrap/charts/{}/Chart.yaml",
            current_directory
                .to_str()
                .expect("Impossible to convert current directory to string"),
            get_helm_path_kubernetes_provider_sub_folder_name(chart.chart_path.helm_path(), HelmChartType::Shared,),
            AlertConfigChart::chart_name(),
        );

        // execute
        let values_file = std::fs::File::open(&chart_path);

        // verify:
        assert!(values_file.is_ok(), "Chart directory should exist: `{chart_path}`");
    }

    /// Makes sure chart values file exists.
    #[test]
    fn alert_config_metrics_chart_values_file_exists_test() {
        // setup:
        let chart = AlertConfigChart::new(
            HelmAction::Deploy,
            HelmChartNamespaces::Prometheus,
            None,
            "cluster-name",
            Some(AlertManagerConfig {
                enabled: true,
                default_rule_labels: None,
                spec_config_secret: None,
                spec_external_url: None,
                receivers: vec![],
                alerts: vec![],
                config_name: None,
            }),
        );

        let current_directory = env::current_dir().expect("Impossible to get current directory");
        {
            let provider_kind = Kind::Eks;
            let chart_values_path = format!(
                "{}/lib/{}/bootstrap/chart_values/{}.yaml",
                current_directory
                    .to_str()
                    .expect("Impossible to convert current directory to string"),
                get_helm_path_kubernetes_provider_sub_folder_name(
                    chart.chart_values_path.helm_path(),
                    HelmChartType::Shared
                ),
                AlertConfigChart::chart_name(),
            );

            // execute
            let values_file = std::fs::File::open(&chart_values_path);

            // verify:
            assert!(
                values_file.is_ok(),
                "Chart values {provider_kind} file should exist: `{chart_values_path}`"
            );
        }
    }

    /// Make sure rust code deosn't set a value not declared inside values file.
    /// All values should be declared / set in values file unless it needs to be injected via rust code.
    #[test]
    fn alert_config_chart_rust_overridden_values_exists_in_values_yaml_test() {
        // setup:
        let chart = AlertConfigChart::new(
            HelmAction::Deploy,
            HelmChartNamespaces::Prometheus,
            None,
            "cluster-name",
            Some(AlertManagerConfig {
                enabled: true,
                default_rule_labels: None,
                spec_config_secret: None,
                spec_external_url: None,
                receivers: vec![],
                alerts: vec![],
                config_name: None,
            }),
        );
        let common_chart = chart.to_common_helm_chart().unwrap();

        {
            // execute:
            let missing_fields = get_helm_values_set_in_code_but_absent_in_values_file(
                common_chart.clone(),
                format!(
                    "/lib/{}/bootstrap/chart_values/{}.yaml",
                    get_helm_path_kubernetes_provider_sub_folder_name(
                        chart.chart_values_path.helm_path(),
                        HelmChartType::Shared,
                    ),
                    AlertConfigChart::chart_name()
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
}

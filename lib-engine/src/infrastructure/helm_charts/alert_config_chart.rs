use crate::errors::CommandError;
use crate::helm::{
    ChartInfo, ChartInstallationChecker, ChartSetValue, CommonChart, HelmAction, HelmChartError, HelmChartNamespaces,
};
use crate::infrastructure::helm_charts::{
    HelmChartDirectoryLocation, HelmChartPath, HelmChartValuesFilePath, ToCommonHelmChart,
};
use crate::io_models::metrics::{AlertConfigAlert, AlertConfigReceiver, AlertManagerConfig, AlertTarget};
use crate::utilities::to_short_id;
use itertools::Itertools;
use kube::Client;
use std::collections::HashMap;
use uuid::Uuid;

const ALERT_RECEIVER_PREFIX_K8S: &str = "qovery-alert-receiver";
const ALERT_RECEIVER_PREFIX_LABEL: &str = "qovery_alert_receiver";
const ALERT_PREFIX: &str = "qovery-alert";
const DEFAULT_ALERT_MANAGER_CONFIG_NAME: &str = "qovery-alert-manager-config";

pub struct AlertConfigChart {
    action: HelmAction,
    alert_config: Option<AlertManagerConfig>,
    chart_path: HelmChartPath,
    chart_values_path: HelmChartValuesFilePath,
    prometheus_namespace: HelmChartNamespaces,
    _cluster_name: String,
    organization_id: Uuid,
}

impl AlertConfigChart {
    pub fn new(
        action: HelmAction,
        prometheus_namespace: HelmChartNamespaces,
        chart_prefix_path: Option<&str>,
        cluster_name: &str,
        alert_config: Option<AlertManagerConfig>,
        organization_id: Uuid,
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
            organization_id,
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
            values.push(ChartSetValue {
                key: "alertManagerConfigName".to_string(),
                value: alert_config
                    .config_name
                    .as_deref()
                    .unwrap_or(DEFAULT_ALERT_MANAGER_CONFIG_NAME)
                    .to_string(),
            });

            for (index, receiver) in alert_config.receivers.iter().enumerate() {
                values.extend(build_receiver_values(index, receiver));
            }

            let alerts_by_target = group_alerts_by_target(&alert_config.alerts);

            for (index, (target, target_alerts)) in alerts_by_target.iter().enumerate() {
                let target_values = build_target_values(
                    index,
                    target,
                    target_alerts,
                    &self.prometheus_namespace,
                    &self.organization_id,
                );
                values.extend(target_values.values);
                values_string.extend(target_values.values_string);
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

fn format_receiver_id_k8s(long_id: &Uuid) -> String {
    format!("{}-{}", ALERT_RECEIVER_PREFIX_K8S, to_short_id(long_id))
}

fn format_receiver_id_label(long_id: &Uuid) -> String {
    format!("{}_{}", ALERT_RECEIVER_PREFIX_LABEL, to_short_id(long_id))
}

fn format_alert_id(long_id: &Uuid) -> String {
    format!("{}-{}", ALERT_PREFIX, to_short_id(long_id))
}

fn format_target_id(target: &AlertTarget) -> String {
    let type_suffix = format!("{:?}", target.r#type).to_lowercase();
    format!("{}-{}-{}", ALERT_PREFIX, type_suffix, to_short_id(&target.id))
}

/// Example: "high CPU usage" -> "HighCPUUsage"
fn format_alert_name_prometheus(name: &str) -> Option<String> {
    fn capitalize_first_char(word: &str) -> String {
        let mut chars = word.chars();
        match chars.next() {
            None => String::new(),
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        }
    }

    let formatted = name
        .split(|c: char| c.is_whitespace() || c == '-' || c == '.' || c == '_')
        .filter(|s| !s.is_empty())
        .map(capitalize_first_char)
        .collect::<String>();

    if formatted.is_empty() {
        return None;
    }

    // Prefix with underscore if it doesn't start with a letter
    if formatted.chars().next().map(|c| c.is_alphabetic()) == Some(false) {
        Some(format!("_{formatted}"))
    } else {
        Some(formatted)
    }
}

fn group_alerts_by_target(alerts: &[AlertConfigAlert]) -> HashMap<&AlertTarget, Vec<&AlertConfigAlert>> {
    alerts.iter().into_group_map_by(|alert| &alert.target)
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
                value: format_receiver_id_k8s(long_id),
            },
            ChartSetValue {
                key: format!("receivers[{index}].matcher_label"),
                value: format_receiver_id_label(long_id),
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

fn build_target_values(
    target_index: usize,
    target: &AlertTarget,
    alerts: &[&AlertConfigAlert],
    namespace: &HelmChartNamespaces,
    organization_id: &Uuid,
) -> AlertChartValues {
    let mut values = Vec::new();
    let mut values_string = Vec::new();
    let target_prefix = format!("targets[{target_index}]");

    values.extend([
        ChartSetValue {
            key: format!("{target_prefix}.id"),
            value: format_target_id(target),
        },
        ChartSetValue {
            key: format!("{target_prefix}.target_id"),
            value: target.id.to_string(),
        },
        ChartSetValue {
            key: format!("{target_prefix}.target_type"),
            value: format!("{:?}", target.r#type),
        },
        ChartSetValue {
            key: format!("{target_prefix}.namespace"),
            value: namespace.to_string(),
        },
        ChartSetValue {
            key: format!("{target_prefix}.organization_long_id"),
            value: organization_id.to_string(),
        },
    ]);

    for (alert_index, alert) in alerts.iter().enumerate() {
        let alert_prefix = format!("{target_prefix}.alerts[{alert_index}]");

        values.extend([
            ChartSetValue {
                key: format!("{alert_prefix}.long_id"),
                value: format_alert_id(&alert.long_id),
            },
            ChartSetValue {
                key: format!("{alert_prefix}.alert_long_id"),
                value: alert.long_id.to_string(),
            },
            ChartSetValue {
                key: format!("{alert_prefix}.target_name"),
                value: get_display_name(&alert.target),
            },
            ChartSetValue {
                key: format!("{alert_prefix}.base_env_url"),
                value: get_base_env_url(&alert.target),
            },
            ChartSetValue {
                key: format!("{alert_prefix}.tag"),
                value: alert
                    .tag
                    .as_ref()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "undefined tag".to_string()),
            },
            ChartSetValue {
                key: format!("{alert_prefix}.name"),
                value: format_alert_name_prometheus(&alert.name).unwrap_or_else(|| alert.name.clone()),
            },
            ChartSetValue {
                key: format!("{alert_prefix}.expr"),
                value: alert
                    .expr
                    .replace("\\", "\\\\")
                    .replace("\"", "\\\"")
                    .replace(",", "\\,")
                    .clone(),
            },
            ChartSetValue {
                key: format!("{alert_prefix}.for"),
                value: format!("{}m", alert.for_duration_minutes),
            },
            ChartSetValue {
                key: format!("{alert_prefix}.version"),
                value: alert
                    .version_tag
                    .as_ref()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "undefined".to_string()),
            },
            ChartSetValue {
                key: format!("{alert_prefix}.organization_long_id"),
                value: organization_id.to_string(),
            },
        ]);

        if let Some(summary) = &alert.summary {
            values_string.push(ChartSetValue {
                key: format!("{alert_prefix}.annotations.summary"),
                value: summary.clone(),
            })
        }

        if let Some(severity) = &alert.severity {
            values.push(ChartSetValue {
                key: format!("{alert_prefix}.annotations.severity"),
                value: capitalize(severity),
            })
        }

        if let Some(description) = &alert.description {
            values_string.push(ChartSetValue {
                key: format!("{alert_prefix}.annotations.description"),
                value: description.clone(),
            })
        }

        if let Some(runbook_url) = &alert.runbook_url {
            values.push(ChartSetValue {
                key: format!("{alert_prefix}.annotations.runbook_url"),
                value: runbook_url.clone(),
            })
        }

        values.push(ChartSetValue {
            key: format!("{alert_prefix}.annotations.qovery_alert_display_name"),
            value: alert.name.clone(),
        });

        values_string.extend(alert.labels.iter().map(|(key, value)| ChartSetValue {
            key: format!("{alert_prefix}.labels.{key}"),
            value: value.clone(),
        }));

        for receiver_id in &alert.receivers {
            values_string.push(ChartSetValue {
                key: format!("{alert_prefix}.labels.{}", format_receiver_id_label(receiver_id)),
                value: "true".to_string(),
            });
        }
    }

    AlertChartValues { values, values_string }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
    }
}

fn get_base_env_url(alert_target: &AlertTarget) -> String {
    match (alert_target.environment_id, alert_target.project_id) {
        (Some(environment_id), Some(project_id)) => {
            format!(
                "https://console.qovery.com/organization/{}/project/{}/environment/{}",
                alert_target.organization_id, project_id, environment_id
            )
        }
        _ => String::new(),
    }
}

fn get_display_name(alert_target: &AlertTarget) -> String {
    match &alert_target.environment_name {
        Some(environment_name) => format!("{environment_name}/{}", alert_target.name),
        None => alert_target.name.clone(),
    }
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
    use uuid::Uuid;

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
            Uuid::new_v4(),
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
            Uuid::new_v4(),
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
            Uuid::new_v4(),
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

    #[test]
    fn test_group_alerts_by_target() {
        use crate::io_models::metrics::{AlertConfigAlert, AlertTarget, AlertTargetType};
        use std::collections::HashMap;
        use uuid::Uuid;

        // Create two different targets
        let target1 = AlertTarget {
            id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
            r#type: AlertTargetType::Application,
            name: "serviceA".to_string(),
            organization_id: Default::default(),
            project_id: None,
            project_name: None,
            environment_id: None,
            environment_name: None,
        };
        let target2 = AlertTarget {
            id: Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap(),
            r#type: AlertTargetType::Container,
            name: "serviceA".to_string(),
            organization_id: Default::default(),
            project_id: None,
            project_name: None,
            environment_id: None,
            environment_name: None,
        };

        // Create alerts for target1
        let alert1 = AlertConfigAlert {
            long_id: Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap(),
            name: "Alert1".to_string(),
            expr: "expr1".to_string(),
            for_duration_minutes: 5,
            labels: HashMap::new(),
            summary: None,
            description: None,
            severity: None,
            runbook_url: None,
            receivers: vec![],
            target: target1.clone(),
            version_tag: Some("1".to_string()),
            tag: None,
        };

        let alert2 = AlertConfigAlert {
            long_id: Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb").unwrap(),
            name: "Alert2".to_string(),
            expr: "expr2".to_string(),
            for_duration_minutes: 10,
            labels: HashMap::new(),
            summary: None,
            description: None,
            severity: None,
            runbook_url: None,
            receivers: vec![],
            target: target1.clone(),
            version_tag: Some("1".to_string()),
            tag: None,
        };

        // Create alert for target2
        let alert3 = AlertConfigAlert {
            long_id: Uuid::parse_str("cccccccc-cccc-cccc-cccc-cccccccccccc").unwrap(),
            name: "Alert3".to_string(),
            expr: "expr3".to_string(),
            for_duration_minutes: 15,
            labels: HashMap::new(),
            summary: None,
            description: None,
            severity: None,
            runbook_url: None,
            receivers: vec![],
            target: target2.clone(),
            version_tag: Some("2".to_string()),
            tag: None,
        };

        let alerts = vec![alert1, alert2, alert3];

        // Execute
        let grouped = super::group_alerts_by_target(&alerts);

        // Verify
        assert_eq!(grouped.len(), 2, "Should have 2 different targets");
        assert_eq!(grouped.get(&target1).unwrap().len(), 2, "Target1 should have 2 alerts");
        assert_eq!(grouped.get(&target2).unwrap().len(), 1, "Target2 should have 1 alert");
    }

    #[test]
    fn test_build_target_values() {
        use crate::io_models::metrics::{AlertConfigAlert, AlertTarget, AlertTargetType};
        use std::collections::HashMap;
        use uuid::Uuid;

        let target = AlertTarget {
            id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
            r#type: AlertTargetType::Application,
            name: "serviceA".to_string(),
            organization_id: Default::default(),
            project_id: None,
            project_name: None,
            environment_id: None,
            environment_name: None,
        };

        let receiver_id = Uuid::parse_str("99999999-9999-9999-9999-999999999999").unwrap();

        let mut labels = HashMap::new();
        labels.insert("env".to_string(), "prod".to_string());

        let alert = AlertConfigAlert {
            long_id: Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap(),
            name: "TestAlert".to_string(),
            expr: "up == 0".to_string(),
            for_duration_minutes: 5,
            labels: labels.clone(),
            summary: Some("Service is down".to_string()),
            description: Some("The service has been down for 5 minutes".to_string()),
            severity: Some("Warning".to_string()),
            runbook_url: Some("https://runbook.example.com".to_string()),
            receivers: vec![receiver_id],
            target: target.clone(),
            version_tag: Some("1".to_string()),
            tag: None,
        };

        let alerts = vec![&alert];

        // Execute
        let result = super::build_target_values(0, &target, &alerts, &HelmChartNamespaces::Prometheus, &Uuid::new_v4());

        // Verify target values
        assert!(
            result
                .values
                .iter()
                .any(|v| v.key == "targets[0].id" && v.value.contains("application")),
            "Should contain target id with type"
        );
        assert!(
            result
                .values
                .iter()
                .any(|v| v.key == "targets[0].target_id" && v.value == "11111111-1111-1111-1111-111111111111"),
            "Should contain target UUID"
        );
        assert!(
            result
                .values
                .iter()
                .any(|v| v.key == "targets[0].target_type" && v.value == "Application"),
            "Should contain target type"
        );

        // Verify alert values
        assert!(
            result
                .values
                .iter()
                .any(|v| v.key == "targets[0].alerts[0].name" && v.value == "TestAlert"),
            "Should contain alert name"
        );
        assert!(
            result
                .values
                .iter()
                .any(|v| v.key == "targets[0].alerts[0].alert_long_id"
                    && v.value == "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"),
            "Should contain alert long UUID"
        );
        assert!(
            result
                .values
                .iter()
                .any(|v| v.key == "targets[0].alerts[0].expr" && v.value == "up == 0"),
            "Should contain alert expression"
        );
        assert!(
            result
                .values
                .iter()
                .any(|v| v.key == "targets[0].alerts[0].for" && v.value == "5m"),
            "Should contain alert duration"
        );

        // Verify annotations
        assert!(
            result
                .values_string
                .iter()
                .any(|v| v.key == "targets[0].alerts[0].annotations.summary" && v.value == "Service is down"),
            "Should contain summary annotation"
        );
        assert!(
            result
                .values_string
                .iter()
                .any(|v| v.key == "targets[0].alerts[0].annotations.description"),
            "Should contain description annotation"
        );
        assert!(
            result
                .values
                .iter()
                .any(|v| v.key == "targets[0].alerts[0].annotations.runbook_url"),
            "Should contain runbook_url annotation"
        );

        // Verify labels
        assert!(
            result
                .values_string
                .iter()
                .any(|v| v.key == "targets[0].alerts[0].labels.env" && v.value == "prod"),
            "Should contain custom label"
        );

        // Verify receiver label
        assert!(
            result
                .values_string
                .iter()
                .any(|v| v.key.contains("qovery_alert_receiver") && v.value == "true"),
            "Should contain receiver label"
        );
    }

    #[test]
    fn test_integration_complete_alert_config() {
        use crate::io_models::metrics::{
            AlertConfigAlert, AlertConfigReceiver, AlertManagerConfig, AlertTarget, AlertTargetType,
        };
        use std::collections::HashMap;
        use uuid::Uuid;

        // Create receiver
        let receiver_id = Uuid::parse_str("fa370bf5-d8a0-46fb-9066-e092cf5a37f1").unwrap();
        let receiver = AlertConfigReceiver::SlackConfig {
            long_id: receiver_id,
            name: "TestSlackReceiver".to_string(),
            api_url: "https://hooks.slack.com/test".to_string(),
            channel: "#alerts".to_string(),
        };

        // Create target
        let target = AlertTarget {
            id: Uuid::parse_str("3f50657b-1162-4dde-b706-4d5e937f3c09").unwrap(),
            r#type: AlertTargetType::KubernetesProvider,
            name: "serviceA".to_string(),
            organization_id: Default::default(),
            project_id: None,
            project_name: None,
            environment_id: None,
            environment_name: None,
        };

        // Create alert
        let alert = AlertConfigAlert {
            long_id: Uuid::parse_str("89bf371f-1162-4dde-b706-4d5e937f3c01").unwrap(),
            name: "TestAlert1".to_string(),
            expr: "vector(1) > 0".to_string(),
            for_duration_minutes: 5,
            labels: HashMap::new(),
            summary: Some("CPU usage is too high".to_string()),
            description: Some("This is for a test".to_string()),
            severity: Some("Warning".to_string()),
            runbook_url: None,
            receivers: vec![receiver_id],
            target: target.clone(),
            version_tag: Some("1".to_string()),
            tag: None,
        };

        let alert_config = AlertManagerConfig {
            enabled: true,
            default_rule_labels: None,
            spec_config_secret: None,
            spec_external_url: None,
            receivers: vec![receiver],
            alerts: vec![alert],
            config_name: Some("test-config".to_string()),
        };

        // Execute
        let chart = AlertConfigChart::new(
            HelmAction::Deploy,
            HelmChartNamespaces::Prometheus,
            None,
            "test-cluster",
            Some(alert_config),
            Uuid::new_v4(),
        );

        let common_chart = chart.to_common_helm_chart().unwrap();

        // Verify
        assert_eq!(common_chart.chart_info.action, HelmAction::Deploy);
        assert!(!common_chart.chart_info.values.is_empty(), "Should have values");

        // Verify receiver values
        assert!(
            common_chart
                .chart_info
                .values
                .iter()
                .any(|v| v.key == "receivers[0].name" && v.value == "TestSlackReceiver"),
            "Should contain receiver name"
        );

        // Verify target and alert values
        assert!(
            common_chart
                .chart_info
                .values
                .iter()
                .any(|v| v.key.starts_with("targets[0]")),
            "Should contain target values"
        );
        assert!(
            common_chart
                .chart_info
                .values
                .iter()
                .any(|v| v.key.contains("alerts[0]")),
            "Should contain alert values"
        );
    }

    #[test]
    fn test_edge_case_no_alerts() {
        let alert_config = AlertManagerConfig {
            enabled: true,
            default_rule_labels: None,
            spec_config_secret: None,
            spec_external_url: None,
            receivers: vec![],
            alerts: vec![],
            config_name: None,
        };

        let chart = AlertConfigChart::new(
            HelmAction::Deploy,
            HelmChartNamespaces::Prometheus,
            None,
            "test-cluster",
            Some(alert_config),
            Uuid::new_v4(),
        );

        let common_chart = chart.to_common_helm_chart().unwrap();

        // Should still work with no alerts
        assert_eq!(common_chart.chart_info.action, HelmAction::Deploy);
        assert!(
            common_chart
                .chart_info
                .values
                .iter()
                .any(|v| v.key == "alertManagerConfigName"),
            "Should still set alertManagerConfigName"
        );
    }

    #[test]
    fn test_edge_case_alert_without_optional_fields() {
        use crate::io_models::metrics::{AlertConfigAlert, AlertTarget, AlertTargetType};
        use std::collections::HashMap;
        use uuid::Uuid;

        let target = AlertTarget {
            id: Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),
            r#type: AlertTargetType::Application,
            name: "serviceA".to_string(),
            organization_id: Default::default(),
            project_id: None,
            project_name: None,
            environment_id: None,
            environment_name: None,
        };

        let alert = AlertConfigAlert {
            long_id: Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap(),
            name: "MinimalAlert".to_string(),
            expr: "up == 0".to_string(),
            for_duration_minutes: 5,
            labels: HashMap::new(),
            summary: None,
            description: None,
            severity: Some("Warning".to_string()),
            runbook_url: None,
            receivers: vec![],
            target: target.clone(),
            version_tag: Some("1".to_string()),
            tag: None,
        };

        let alerts = vec![&alert];
        let result = super::build_target_values(0, &target, &alerts, &HelmChartNamespaces::Prometheus, &Uuid::new_v4());

        // Should not have annotation keys if values are None
        assert!(
            !result
                .values
                .iter()
                .any(|v| v.key == "targets[0].alerts[0].annotations.summary"),
            "Should not have summary annotation when None"
        );
        assert!(
            !result
                .values
                .iter()
                .any(|v| v.key == "targets[0].alerts[0].annotations.description"),
            "Should not have description annotation when None"
        );
        assert!(
            !result
                .values
                .iter()
                .any(|v| v.key == "targets[0].alerts[0].annotations.runbook_url"),
            "Should not have runbook_url annotation when None"
        );
    }
}

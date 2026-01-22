use crate::infrastructure::action::metrics_resource_profile::ResourceProfile;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all(serialize = "snake_case", deserialize = "snake_case"))]
pub struct MetricsParameters {
    pub config: MetricsConfiguration,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all(serialize = "snake_case", deserialize = "snake_case"))]
pub enum MetricsConfiguration {
    MetricsInstalledByQovery {
        // INFO (ENG-1986) ATM this field should be filled only for dedicated Qovery internal clusters
        install_prometheus_adapter: bool,
        enable_redundancy: Option<bool>,
        beyla_config: Option<BeylaConfig>,
        alert_config: Option<AlertManagerConfig>,
        #[serde(default)]
        resource_profile: ResourceProfile,
        #[serde(default, rename = "cloud_watch_exporter_config")]
        cloudwatch_exporter_config: CloudWatchExporterConfig,
    },
    AwsS3 {
        region: String,
        bucket_name: String,
        aws_iam_prometheus_role_arn: String,
        endpoint: String,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all(serialize = "snake_case", deserialize = "snake_case"))]
pub struct BeylaConfig {
    pub enabled: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all(serialize = "snake_case", deserialize = "snake_case"))]
pub enum AlertConfigReceiver {
    SlackConfig {
        long_id: Uuid,
        name: String,
        api_url: String,
        channel: String,
    },
    EmailConfig {
        long_id: Uuid,
        name: String,
        to: String,
        from: String,
        smarthost: String,
        auth_username: Option<String>,
        auth_password: Option<String>,
        require_tls: Option<bool>,
        send_resolved: Option<bool>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all(serialize = "SCREAMING_SNAKE_CASE", deserialize = "SCREAMING_SNAKE_CASE"))]
pub enum AlertTargetType {
    KubernetesProvider,
    Environment,
    Application,
    Container,
    Job,
    Cronjob,
    Helm,
    Terraform,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all(serialize = "snake_case", deserialize = "snake_case"))]
pub struct AlertTarget {
    pub id: Uuid,
    pub r#type: AlertTargetType,
    pub name: String,
    pub organization_id: Uuid,
    pub project_id: Option<Uuid>,
    pub project_name: Option<String>,
    pub environment_id: Option<Uuid>,
    pub environment_name: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all(serialize = "snake_case", deserialize = "snake_case"))]
pub struct AlertConfigAlert {
    pub long_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub severity: Option<String>,
    pub expr: String,
    pub for_duration_minutes: i32,
    pub labels: HashMap<String, String>,
    pub summary: Option<String>,
    pub runbook_url: Option<String>,
    #[serde(default)]
    pub receivers: Vec<Uuid>,
    pub target: AlertTarget,
    pub version_tag: Option<String>,
    pub tag: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all(serialize = "snake_case", deserialize = "snake_case"))]
pub struct AlertManagerConfig {
    pub enabled: bool,
    pub default_rule_labels: Option<HashMap<String, String>>,
    pub spec_config_secret: Option<String>,
    pub spec_external_url: Option<String>,
    #[serde(default)]
    pub receivers: Vec<AlertConfigReceiver>,
    #[serde(default)]
    pub alerts: Vec<AlertConfigAlert>,
    pub config_name: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(rename_all(serialize = "snake_case", deserialize = "snake_case"))]
pub struct CloudWatchExporterConfig {
    #[serde(default)]
    pub enabled: bool,
}

#[cfg(test)]
mod tests {
    use crate::infrastructure::action::metrics_resource_profile::ResourceProfile;
    use crate::io_models::metrics::{AlertConfigReceiver, AlertTargetType, MetricsConfiguration, MetricsParameters};
    use std::collections::HashMap;
    use uuid::Uuid;

    #[test]
    fn test_metrics_configs_deserialization() {
        let data = r#"{
        "config": {
          "metrics_installed_by_qovery": {
            "install_prometheus_adapter": false,
            "enable_redundancy": true,
            "beyla_config": {
              "enabled": true
            },
            "resource_profile": "HIGH",
            "alert_config": {
              "enabled":true,
              "default_rule_labels":{"A":"B"},
              "spec_config_secret":"configSecret",
              "spec_external_url":"externalUrl",
              "receivers":
              [
                 {
                    "slack_config":
                    {
                        "long_id":"4f50657b-1162-4dde-b706-4d5e937f3c09",
                        "name":"receiver 1",
                        "api_url":"url1",
                        "channel":"channel 1"
                    }
                }
              ],
              "alerts":
              [
                {
                  "long_id":"4f50657b-1162-4dde-b706-4d5e937f3c01",
                  "name":"alert 1",
                  "expr":"expr 1",
                  "for_duration_minutes": 5,
                  "labels":{"label 1":"v1"},
                  "summary":"summary 1",
                  "description":"description 1",
                  "runbook_url":"runbookUrl 1",
                  "target": {
                    "id":"4f50657b-1162-4dde-b706-4d5e937f3c02",
                    "type":"APPLICATION",
                    "name": "a service",
                    "organization_id":"4f50657b-1162-4dde-b706-4d5e937f3c09"
                  },
                  "version_tag": "2025-10-24T12:34:18.232424Z"
                }
              ],
              "config_name":"config Name"
            }
          }
        }
      }"#
        .to_string();

        let metrics_parameters: MetricsParameters = serde_json::from_str(data.as_str()).unwrap();
        let MetricsConfiguration::MetricsInstalledByQovery {
            install_prometheus_adapter,
            enable_redundancy,
            beyla_config,
            alert_config,
            resource_profile,
            cloudwatch_exporter_config,
        } = metrics_parameters.config
        else {
            panic!("Expected MetricsInstalledByQovery variant");
        };

        // Check basic fields
        assert!(!install_prometheus_adapter);
        assert_eq!(enable_redundancy, Some(true));
        assert_eq!(resource_profile, ResourceProfile::High);

        // Check beyla config
        let beyla = beyla_config.expect("beyla_config should be present");
        assert!(beyla.enabled);

        // Check alert config
        let alert_cfg = alert_config.expect("alert_config should be present");
        assert!(alert_cfg.enabled);
        assert_eq!(
            alert_cfg.default_rule_labels,
            Some(HashMap::from([("A".to_string(), "B".to_string())]))
        );
        assert_eq!(alert_cfg.spec_config_secret, Some("configSecret".to_string()));
        assert_eq!(alert_cfg.spec_external_url, Some("externalUrl".to_string()));
        assert_eq!(alert_cfg.config_name, Some("config Name".to_string()));

        // Check receivers
        assert_eq!(alert_cfg.receivers.len(), 1);
        let AlertConfigReceiver::SlackConfig {
            long_id,
            name,
            api_url,
            channel,
        } = &alert_cfg.receivers[0]
        else {
            panic!("Expected SlackConfig variant");
        };
        assert_eq!(*long_id, Uuid::parse_str("4f50657b-1162-4dde-b706-4d5e937f3c09").unwrap());
        assert_eq!(name, "receiver 1");
        assert_eq!(api_url, "url1");
        assert_eq!(channel, "channel 1");

        // Check alerts
        assert_eq!(alert_cfg.alerts.len(), 1);
        let alert = &alert_cfg.alerts[0];
        assert_eq!(alert.long_id, Uuid::parse_str("4f50657b-1162-4dde-b706-4d5e937f3c01").unwrap());
        assert_eq!(alert.name, "alert 1");
        assert_eq!(alert.expr, "expr 1");
        assert_eq!(alert.for_duration_minutes, 5);
        assert_eq!(alert.labels, HashMap::from([("label 1".to_string(), "v1".to_string())]));
        assert_eq!(alert.summary, Some("summary 1".to_string()));
        assert_eq!(alert.description, Some("description 1".to_string()));
        assert_eq!(alert.runbook_url, Some("runbookUrl 1".to_string()));
        assert_eq!(
            alert.target.id,
            Uuid::parse_str("4f50657b-1162-4dde-b706-4d5e937f3c02").unwrap()
        );
        assert_eq!(alert.target.r#type, AlertTargetType::Application);
        assert_eq!(alert.version_tag, Some("2025-10-24T12:34:18.232424Z".to_string()));

        // Check cloudwatch_exporter_config
        assert!(!cloudwatch_exporter_config.enabled);
    }

    #[test]
    fn test_email_config_deserialization() {
        let data = r#"{
        "config": {
          "metrics_installed_by_qovery": {
            "install_prometheus_adapter": false,
            "resource_profile": "NORMAL",
            "alert_config": {
              "enabled": true,
              "receivers": [
                {
                  "email_config": {
                    "long_id": "5f60758c-1263-4eef-c817-5e6f938f4d10",
                    "name": "ops-email",
                    "to": "ops@example.com",
                    "from": "alerts@example.com",
                    "smarthost": "smtp.gmail.com:587",
                    "auth_username": "alerts@example.com",
                    "auth_password": "app-password",
                    "require_tls": true,
                    "send_resolved": true
                  }
                }
              ],
              "alerts": []
            }
          }
        }
      }"#;

        let metrics_parameters: MetricsParameters = serde_json::from_str(data).unwrap();
        let MetricsConfiguration::MetricsInstalledByQovery { alert_config, .. } = metrics_parameters.config else {
            panic!("Expected MetricsInstalledByQovery variant");
        };

        let alert_cfg = alert_config.expect("alert_config should be present");
        assert_eq!(alert_cfg.receivers.len(), 1);

        let AlertConfigReceiver::EmailConfig {
            long_id,
            name,
            to,
            from,
            smarthost,
            auth_username,
            auth_password,
            require_tls,
            send_resolved,
        } = &alert_cfg.receivers[0]
        else {
            panic!("Expected EmailConfig variant");
        };

        assert_eq!(*long_id, Uuid::parse_str("5f60758c-1263-4eef-c817-5e6f938f4d10").unwrap());
        assert_eq!(name, "ops-email");
        assert_eq!(to, "ops@example.com");
        assert_eq!(from, "alerts@example.com");
        assert_eq!(smarthost, "smtp.gmail.com:587");
        assert_eq!(auth_username, &Some("alerts@example.com".to_string()));
        assert_eq!(auth_password, &Some("app-password".to_string()));
        assert_eq!(require_tls, &Some(true));
        assert_eq!(send_resolved, &Some(true));
    }

    #[test]
    fn test_email_config_minimal_deserialization() {
        let data = r#"{
        "config": {
          "metrics_installed_by_qovery": {
            "install_prometheus_adapter": false,
            "resource_profile": "NORMAL",
            "alert_config": {
              "enabled": true,
              "receivers": [
                {
                  "email_config": {
                    "long_id": "6f70859d-2374-5efe-d928-6f7a049e5e21",
                    "name": "minimal-email",
                    "to": "alerts@example.com",
                    "from": "noreply@example.com",
                    "smarthost": "smtp.example.com:25"
                  }
                }
              ],
              "alerts": []
            }
          }
        }
      }"#;

        let metrics_parameters: MetricsParameters = serde_json::from_str(data).unwrap();
        let MetricsConfiguration::MetricsInstalledByQovery { alert_config, .. } = metrics_parameters.config else {
            panic!("Expected MetricsInstalledByQovery variant");
        };

        let alert_cfg = alert_config.expect("alert_config should be present");
        assert_eq!(alert_cfg.receivers.len(), 1);

        let AlertConfigReceiver::EmailConfig {
            long_id,
            name,
            to,
            from,
            smarthost,
            auth_username,
            auth_password,
            require_tls,
            send_resolved,
        } = &alert_cfg.receivers[0]
        else {
            panic!("Expected EmailConfig variant");
        };

        assert_eq!(*long_id, Uuid::parse_str("6f70859d-2374-5efe-d928-6f7a049e5e21").unwrap());
        assert_eq!(name, "minimal-email");
        assert_eq!(to, "alerts@example.com");
        assert_eq!(from, "noreply@example.com");
        assert_eq!(smarthost, "smtp.example.com:25");
        assert_eq!(auth_username, &None);
        assert_eq!(auth_password, &None);
        assert_eq!(require_tls, &None);
        assert_eq!(send_resolved, &None);
    }
}

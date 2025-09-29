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
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all(serialize = "snake_case", deserialize = "snake_case"))]
pub struct AlertConfigAlert {
    pub long_id: Uuid,
    pub name: String,
    pub expr: String,
    #[serde(rename = "for")]
    pub r#for: String,
    pub labels: HashMap<String, String>,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub runbook_url: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all(serialize = "snake_case", deserialize = "snake_case"))]
pub struct AlertManagerConfig {
    pub enabled: bool,
    pub default_rule_labels: Option<HashMap<String, String>>,
    pub spec_config_secret: Option<String>,
    pub spec_external_url: Option<String>,
    pub receivers: Vec<AlertConfigReceiver>,
    pub alerts: Vec<AlertConfigAlert>,
    pub config_name: Option<String>,
}

use serde::{Deserialize, Serialize};

/// `PlatformComponentsOnly` means: do not run any cluster infrastructure lifecycle action
/// (create/pause/delete/restart); apply only the platform Helm units provided in
/// `EngineV2Options::platform_helm_units`.
#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub enum ExecutionMode {
    #[serde(rename = "platform_components_only")]
    PlatformComponentsOnly,
    #[serde(other)]
    Unknown,
}

/// A complete, self-contained Helm execution input compiled by q-core.
/// The engine must use only these resolved references and values; it must not resolve
/// chart/image versions or compute values itself.
#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct PlatformHelmUnit {
    pub key: String,
    pub action: PlatformHelmUnitAction,
    pub release_name: String,
    pub namespace: String,
    pub chart: PlatformHelmChartSource,
    pub values_yaml: String,
    #[serde(default)]
    pub images: Vec<PlatformImageSnapshot>,
}

impl std::fmt::Debug for PlatformHelmUnit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlatformHelmUnit")
            .field("key", &self.key)
            .field("action", &self.action)
            .field("release_name", &self.release_name)
            .field("namespace", &self.namespace)
            .field("chart", &self.chart)
            .field("values_yaml", &"<redacted>")
            .field("images", &self.images)
            .finish()
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub enum PlatformHelmUnitAction {
    #[serde(rename = "CREATE")]
    Create,
    /// Forward-compat catch-all: must produce a structured error at execution.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct PlatformHelmChartSource {
    pub repository: String,
    pub name: String,
    pub version: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct PlatformImageSnapshot {
    pub key: String,
    pub repository: String,
    pub tag: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
pub struct PlatformExecutionResult {
    pub schema_version: u32,
    pub execution_id: String,
    pub units: Vec<PlatformUnitResult>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
pub struct PlatformUnitResult {
    pub key: String,
    pub status: PlatformUnitStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<PlatformUnitErrorCode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl PlatformUnitResult {
    pub fn succeeded(key: &str) -> Self {
        PlatformUnitResult {
            key: key.to_string(),
            status: PlatformUnitStatus::Succeeded,
            error_code: None,
            message: None,
        }
    }

    pub fn failed(key: &str, error_code: PlatformUnitErrorCode, message: &str) -> Self {
        PlatformUnitResult {
            key: key.to_string(),
            status: PlatformUnitStatus::Failed,
            error_code: Some(error_code),
            message: Some(truncate_message(message)),
        }
    }

    pub fn skipped(key: &str, reason: &str) -> Self {
        PlatformUnitResult {
            key: key.to_string(),
            status: PlatformUnitStatus::Skipped,
            error_code: None,
            message: Some(truncate_message(reason)),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, Eq, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PlatformUnitStatus {
    Succeeded,
    Failed,
    Skipped,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, Eq, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PlatformUnitErrorCode {
    InvalidPayload,
    UnsupportedSchemaVersion,
    ForbiddenAction,
    ChartFetchFailed,
    HelmFailed,
    Timeout,
    Internal,
}

/// Kubernetes truncates termination messages at 4096 bytes: keep individual messages short so
/// the whole JSON stays well under the limit even with several units.
const MAX_RESULT_MESSAGE_CHARS: usize = 300;

fn truncate_message(message: &str) -> String {
    if message.chars().count() <= MAX_RESULT_MESSAGE_CHARS {
        return message.to_string();
    }
    let truncated: String = message.chars().take(MAX_RESULT_MESSAGE_CHARS).collect();
    format!("{truncated}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Wire format produced by q-core (`ClusterOperatorService`): snake_case fields, values_yaml
    // as a raw YAML string, action as an uppercase string.
    const Q_CORE_UNIT_JSON: &str = r#"{
        "key": "cluster-agent",
        "action": "CREATE",
        "release_name": "cluster-agent",
        "namespace": "qovery",
        "chart": {
            "repository": "https://helm.qovery.com",
            "name": "qovery-cluster-agent",
            "version": "0.1.0"
        },
        "values_yaml": "image:\n  tag: \"0.1.0-poc\"\nenvironmentVariables:\n  CLUSTER_JWT_TOKEN: \"secret-token\"\n",
        "images": [{"key": "cluster-agent", "repository": "public.ecr.aws/qovery/cluster-agent", "tag": "0.1.0-poc"}]
    }"#;

    #[test]
    fn deserialize_q_core_platform_helm_unit() {
        let unit: PlatformHelmUnit = serde_json::from_str(Q_CORE_UNIT_JSON).unwrap();
        assert_eq!(unit.key, "cluster-agent");
        assert_eq!(unit.action, PlatformHelmUnitAction::Create);
        assert_eq!(unit.release_name, "cluster-agent");
        assert_eq!(unit.namespace, "qovery");
        assert_eq!(unit.chart.repository, "https://helm.qovery.com");
        assert_eq!(unit.chart.name, "qovery-cluster-agent");
        assert_eq!(unit.chart.version, "0.1.0");
        assert!(unit.values_yaml.contains("CLUSTER_JWT_TOKEN"));
        assert_eq!(unit.images.len(), 1);
        assert_eq!(unit.images[0].tag, "0.1.0-poc");
    }

    #[test]
    fn execution_mode_deserializes_from_q_core_wire_value() {
        let mode: ExecutionMode = serde_json::from_str(r#""platform_components_only""#).unwrap();
        assert_eq!(mode, ExecutionMode::PlatformComponentsOnly);
    }

    #[test]
    fn unknown_execution_mode_and_action_map_to_unknown_not_an_error() {
        let mode: ExecutionMode = serde_json::from_str(r#""some_future_mode""#).unwrap();
        assert_eq!(mode, ExecutionMode::Unknown);
        let json = Q_CORE_UNIT_JSON.replace("\"CREATE\"", "\"DESTROY\"");
        let unit: PlatformHelmUnit = serde_json::from_str(&json).unwrap();
        assert_eq!(unit.action, PlatformHelmUnitAction::Unknown);
    }

    #[test]
    fn debug_redacts_values_yaml() {
        let unit: PlatformHelmUnit = serde_json::from_str(Q_CORE_UNIT_JSON).unwrap();
        let debug = format!("{unit:?}");
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("secret-token"));
    }

    #[test]
    fn metadata_deserializes_q_core_operator_payload_and_ignores_extra_keys() {
        use crate::io_models::context::Metadata;

        // Shape produced by q-core `withClusterOperatorPlatformComponentsMetadata`;
        // the operator-transport key is unknown to the engine and must be ignored.
        let json = format!(
            r#"{{
                "dry_run_deploy": false,
                "cluster_operator_engine_v2": true,
                "engine_v2_options": {{
                    "schema_version": "1",
                    "execution_mode": "platform_components_only",
                    "platform_helm_units": [{Q_CORE_UNIT_JSON}]
                }}
            }}"#
        );
        let metadata: Metadata = serde_json::from_str(&json).unwrap();
        let engine_v2_options = metadata.engine_v2_options.unwrap();
        assert_eq!(engine_v2_options.execution_mode, ExecutionMode::PlatformComponentsOnly);
        let units = engine_v2_options.platform_helm_units;
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].key, "cluster-agent");
    }

    #[test]
    fn legacy_metadata_without_operator_fields_still_deserializes() {
        use crate::io_models::context::Metadata;

        let metadata: Metadata = serde_json::from_str(r#"{"dry_run_deploy": true}"#).unwrap();
        assert_eq!(metadata.engine_v2_options, None);
    }

    #[test]
    fn platform_execution_result_serializes_the_termination_message_contract() {
        // This JSON is the wire contract read by the operator from the pod termination message
        // and relayed verbatim to q-core: field names and enum values must stay stable.
        let result = PlatformExecutionResult {
            schema_version: 1,
            execution_id: "exec-1".to_string(),
            units: vec![
                PlatformUnitResult::succeeded("cluster-agent"),
                PlatformUnitResult::failed("shell-agent", PlatformUnitErrorCode::HelmFailed, "helm upgrade failed"),
                PlatformUnitResult::skipped("loki", "UPSTREAM_FAILED"),
            ],
        };
        let json = serde_json::to_string(&result).unwrap();
        assert_eq!(
            json,
            r#"{"schema_version":1,"execution_id":"exec-1","units":[{"key":"cluster-agent","status":"SUCCEEDED"},{"key":"shell-agent","status":"FAILED","error_code":"HELM_FAILED","message":"helm upgrade failed"},{"key":"loki","status":"SKIPPED","message":"UPSTREAM_FAILED"}]}"#
        );
    }

    #[test]
    fn platform_result_messages_are_truncated_under_the_termination_message_limit() {
        let long_message = "x".repeat(5000);
        let result = PlatformExecutionResult {
            schema_version: 1,
            execution_id: "exec-1".to_string(),
            units: vec![PlatformUnitResult::failed(
                "cluster-agent",
                PlatformUnitErrorCode::Internal,
                &long_message,
            )],
        };
        let json = serde_json::to_string(&result).unwrap();
        // Kubernetes truncates termination messages at 4096 bytes; the whole JSON must fit.
        assert!(json.len() < 4096, "termination message JSON too large: {} bytes", json.len());
    }
}

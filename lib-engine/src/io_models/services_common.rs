use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::environment::models::port::Port;
use crate::web_utils::validate_http_header_name;

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
pub enum Protocol {
    HTTP,
    GRPC,
    TCP,
    UDP,
}

impl Protocol {
    pub fn is_layer4(&self) -> bool {
        matches!(self, Protocol::TCP | Protocol::UDP)
    }
    pub fn is_http_layer(&self) -> bool {
        matches!(self, Protocol::HTTP | Protocol::GRPC)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
pub struct PortIo {
    pub long_id: Uuid,
    pub port: u16,
    pub is_default: bool,
    pub name: String,
    pub publicly_accessible: bool,
    pub protocol: Protocol,
    pub service_name: Option<String>,
    pub namespace: Option<String>,

    // Override the default matching path. It makes sense only for HTTP and GRPC protocols
    #[serde(default)]
    pub path: Option<String>,
    // Rewrite the path. It makes sense only for HTTP and GRPC protocols
    #[serde(default)]
    pub path_rewrite: Option<String>,
}

impl PortIo {
    pub fn to_port_domain(&self) -> Port {
        Port::from(self.clone())
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
pub struct GitCredentials {
    pub login: String,
    pub access_token: String,
    pub expired_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash, Default)]
pub enum GatewayApiStickySessionType {
    #[default]
    Cookie,
    Header {
        name: String,
    },
    #[serde(rename = "SourceIP")]
    SourceIp,
}

pub fn default_gateway_api_sticky_session_type() -> GatewayApiStickySessionType {
    GatewayApiStickySessionType::Cookie
}

pub fn default_gateway_api_escaped_slashes_action() -> GatewayApiEscapedSlashesAction {
    GatewayApiEscapedSlashesAction::UnescapeAndRedirect
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash, Default)]
pub enum GatewayApiEscapedSlashesAction {
    KeepUnchanged,      // Preserve %2F as-is in the upstream path.
    RejectRequest,      // Reject requests containing escaped slashes.
    UnescapeAndForward, // Decode %2F to / and forward upstream.
    #[default]
    UnescapeAndRedirect, // Decode %2F and redirect client to normalized path.
}

#[derive(Deserialize)]
#[serde(untagged)]
enum GatewayApiStickySessionTypeInput {
    Explicit(GatewayApiStickySessionType),
    LegacyHeaderName(String),
}

pub fn deserialize_gateway_api_sticky_session_type<'de, D>(
    deserializer: D,
) -> Result<GatewayApiStickySessionType, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let input = Option::<GatewayApiStickySessionTypeInput>::deserialize(deserializer)?;

    Ok(match input {
        None => GatewayApiStickySessionType::Cookie,
        Some(GatewayApiStickySessionTypeInput::Explicit(value)) => match value {
            GatewayApiStickySessionType::Header { name } => {
                validate_http_header_name(&name).map_err(serde::de::Error::custom)?;
                GatewayApiStickySessionType::Header { name }
            }
            other => other,
        },
        Some(GatewayApiStickySessionTypeInput::LegacyHeaderName(value)) => match value.as_str() {
            "Cookie" | "cookie" => GatewayApiStickySessionType::Cookie,
            "SourceIP" => GatewayApiStickySessionType::SourceIp,
            name => {
                if let Ok(parsed) = serde_json::from_str::<GatewayApiStickySessionType>(name) {
                    return Ok(match parsed {
                        GatewayApiStickySessionType::Header { name } => {
                            validate_http_header_name(&name).map_err(serde::de::Error::custom)?;
                            GatewayApiStickySessionType::Header { name }
                        }
                        other => other,
                    });
                }
                validate_http_header_name(name).map_err(serde::de::Error::custom)?;
                tracing::warn!(
                    "legacy string sticky session header value is deprecated; prefer explicit format: {{\"Header\":{{\"name\":\"...\"}}}}"
                );
                GatewayApiStickySessionType::Header { name: name.to_string() }
            }
        },
    })
}

#[cfg(test)]
mod tests {
    use super::{GatewayApiStickySessionType, deserialize_gateway_api_sticky_session_type};
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct TestSettings {
        #[serde(
            default,
            alias = "network.gateway_api.sticky_session_type",
            alias = "network.gateway_api.sticky_session_header",
            deserialize_with = "deserialize_gateway_api_sticky_session_type"
        )]
        network_gateway_api_sticky_session_type: GatewayApiStickySessionType,
    }

    #[test]
    fn sticky_session_type_defaults_to_cookie() {
        let parsed: TestSettings = serde_json::from_str("{}").expect("settings should parse");
        assert_eq!(
            parsed.network_gateway_api_sticky_session_type,
            GatewayApiStickySessionType::Cookie
        );
    }

    #[test]
    fn sticky_session_type_supports_legacy_header_name() {
        let parsed: TestSettings =
            serde_json::from_str(r#"{"network.gateway_api.sticky_session_header":"Mcp-Session-Id"}"#)
                .expect("settings should parse");
        assert_eq!(
            parsed.network_gateway_api_sticky_session_type,
            GatewayApiStickySessionType::Header {
                name: "Mcp-Session-Id".to_string(),
            }
        );
    }

    #[test]
    fn sticky_session_type_supports_source_ip() {
        let parsed: TestSettings = serde_json::from_str(r#"{"network.gateway_api.sticky_session_type":"SourceIP"}"#)
            .expect("settings should parse");
        assert_eq!(
            parsed.network_gateway_api_sticky_session_type,
            GatewayApiStickySessionType::SourceIp
        );
    }

    #[test]
    fn sticky_session_type_supports_header_object() {
        let parsed: TestSettings =
            serde_json::from_str(r#"{"network.gateway_api.sticky_session_type":{"Header":{"name":"Mcp-Session-Id"}}}"#)
                .expect("settings should parse");
        assert_eq!(
            parsed.network_gateway_api_sticky_session_type,
            GatewayApiStickySessionType::Header {
                name: "Mcp-Session-Id".to_string(),
            }
        );
    }

    #[test]
    fn sticky_session_type_supports_legacy_header_name_string() {
        let parsed: TestSettings =
            serde_json::from_str(r#"{"network.gateway_api.sticky_session_type":"Mcp-Session-Id"}"#)
                .expect("settings should parse");
        assert_eq!(
            parsed.network_gateway_api_sticky_session_type,
            GatewayApiStickySessionType::Header {
                name: "Mcp-Session-Id".to_string(),
            }
        );
    }

    #[test]
    fn sticky_session_type_rejects_json_encoded_string() {
        let parsed: Result<TestSettings, _> =
            serde_json::from_str(r#"{"network.gateway_api.sticky_session_type":"{\"Header\":\"X-Benjamin-Test\"}"}"#);
        assert!(parsed.is_err());
    }

    #[test]
    fn sticky_session_type_supports_json_string_with_header_object() {
        let parsed: TestSettings = serde_json::from_str(
            r#"{"network.gateway_api.sticky_session_type":"{\"Header\":{\"name\":\"X-Benjamin-Test\"}}"}"#,
        )
        .expect("settings should parse");
        assert_eq!(
            parsed.network_gateway_api_sticky_session_type,
            GatewayApiStickySessionType::Header {
                name: "X-Benjamin-Test".to_string(),
            }
        );
    }

    #[test]
    fn sticky_session_type_rejects_invalid_header_name() {
        let parsed: Result<TestSettings, _> =
            serde_json::from_str(r#"{"network.gateway_api.sticky_session_type":{"Header":{"name":"X Invalid"}}}"#);
        assert!(parsed.is_err());
    }
}

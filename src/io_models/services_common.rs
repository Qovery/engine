use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::environment::models::port::Port;

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
        Some(GatewayApiStickySessionTypeInput::Explicit(value)) => value,
        Some(GatewayApiStickySessionTypeInput::LegacyHeaderName(value)) => match value.as_str() {
            "Cookie" | "cookie" => GatewayApiStickySessionType::Cookie,
            "SourceIP" => GatewayApiStickySessionType::SourceIp,
            name => GatewayApiStickySessionType::Header { name: name.to_string() },
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
}

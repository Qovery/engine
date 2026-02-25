use super::SecretsManagerConversionError;
use std::collections::HashMap;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum GcpAuthenticationMode {
    Automatic,
    JsonCredentials { content: String },
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum GcpAuthenticationModeKey {
    Automatic,
    JsonCredentials,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct GcpConnection {
    pub region: String,
    pub project_id: String,
    pub authentication_mode: GcpAuthenticationMode,
}

impl TryFrom<&str> for GcpAuthenticationModeKey {
    type Error = SecretsManagerConversionError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "AUTOMATIC" => Ok(Self::Automatic),
            "JSON_CREDENTIALS" => Ok(Self::JsonCredentials),
            _ => Err(SecretsManagerConversionError {
                message: format!("unknown GCP authentication mode: {}", s),
            }),
        }
    }
}

impl GcpConnection {
    pub fn try_parse(
        endpoint: &HashMap<String, String>,
        auth_mode: &str,
        authentication: &HashMap<String, String>,
    ) -> Result<GcpConnection, SecretsManagerConversionError> {
        let region = endpoint
            .get("region")
            .ok_or_else(|| SecretsManagerConversionError {
                message: "missing endpoint.region for GCP_SECRETS_MANAGER".to_string(),
            })?
            .clone();

        let project_id = endpoint
            .get("project_id")
            .ok_or_else(|| SecretsManagerConversionError {
                message: "missing endpoint.project_id for GCP_SECRETS_MANAGER".to_string(),
            })?
            .clone();

        let authentication_mode = GcpAuthenticationMode::try_parse(auth_mode, authentication)?;

        Ok(GcpConnection {
            region,
            project_id,
            authentication_mode,
        })
    }
}

impl GcpAuthenticationMode {
    pub fn try_parse(
        mode: &str,
        auth: &HashMap<String, String>,
    ) -> Result<GcpAuthenticationMode, SecretsManagerConversionError> {
        let auth_mode = GcpAuthenticationModeKey::try_from(mode)?;
        match auth_mode {
            GcpAuthenticationModeKey::Automatic => Ok(GcpAuthenticationMode::Automatic),
            GcpAuthenticationModeKey::JsonCredentials => {
                let content = auth
                    .get("content")
                    .ok_or_else(|| SecretsManagerConversionError {
                        message: "missing authentication.content for JSON_CREDENTIALS mode".to_string(),
                    })?
                    .clone();
                Ok(GcpAuthenticationMode::JsonCredentials { content })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn auth_map(entries: &[(&str, &str)]) -> HashMap<String, String> {
        entries.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn test_authentication_mode_key_valid_modes() {
        assert_eq!(
            GcpAuthenticationModeKey::try_from("AUTOMATIC").unwrap(),
            GcpAuthenticationModeKey::Automatic
        );
        assert_eq!(
            GcpAuthenticationModeKey::try_from("JSON_CREDENTIALS").unwrap(),
            GcpAuthenticationModeKey::JsonCredentials
        );
    }

    #[test]
    fn test_authentication_mode_key_unknown_mode() {
        let err = GcpAuthenticationModeKey::try_from("OAUTH2").unwrap_err();
        assert!(err.message.contains("unknown GCP authentication mode: OAUTH2"));
    }

    #[test]
    fn test_parse_authentication_automatic() {
        let result = GcpAuthenticationMode::try_parse("AUTOMATIC", &auth_map(&[])).unwrap();
        assert_eq!(result, GcpAuthenticationMode::Automatic);
    }

    #[test]
    fn test_parse_authentication_json_credentials() {
        let auth = auth_map(&[("content", r#"{"type":"service_account","project_id":"test"}"#)]);
        let result = GcpAuthenticationMode::try_parse("JSON_CREDENTIALS", &auth).unwrap();
        assert_eq!(
            result,
            GcpAuthenticationMode::JsonCredentials {
                content: r#"{"type":"service_account","project_id":"test"}"#.to_string()
            }
        );
    }

    #[test]
    fn test_parse_authentication_json_credentials_missing_content() {
        let err = GcpAuthenticationMode::try_parse("JSON_CREDENTIALS", &auth_map(&[])).unwrap_err();
        assert!(err.message.contains("missing authentication.content"));
    }

    #[test]
    fn test_parse_authentication_unknown_mode() {
        let err = GcpAuthenticationMode::try_parse("KERBEROS", &auth_map(&[])).unwrap_err();
        assert!(err.message.contains("unknown GCP authentication mode: KERBEROS"));
    }

    #[test]
    fn test_gcp_connection_with_automatic_mode() {
        let conn = GcpConnection {
            region: "europe-west1".to_string(),
            project_id: "my-project".to_string(),
            authentication_mode: GcpAuthenticationMode::Automatic,
        };
        assert_eq!(conn.region, "europe-west1");
        assert_eq!(conn.project_id, "my-project");
        assert_eq!(conn.authentication_mode, GcpAuthenticationMode::Automatic);
    }

    #[test]
    fn test_gcp_connection_with_json_credentials() {
        let content = r#"{"type":"service_account","project_id":"test"}"#;
        let conn = GcpConnection {
            region: "us-central1".to_string(),
            project_id: "test-project".to_string(),
            authentication_mode: GcpAuthenticationMode::JsonCredentials {
                content: content.to_string(),
            },
        };
        match conn.authentication_mode {
            GcpAuthenticationMode::JsonCredentials { content: c } => assert_eq!(c, content),
            _ => panic!("Expected JsonCredentials"),
        }
    }

    #[test]
    fn test_gcp_connection_equality() {
        let conn_a = GcpConnection {
            region: "europe-west1".to_string(),
            project_id: "my-project".to_string(),
            authentication_mode: GcpAuthenticationMode::Automatic,
        };
        let conn_b = conn_a.clone();
        assert_eq!(conn_a, conn_b);
    }

    #[test]
    fn test_parse_gcp_connection_with_automatic() {
        let endpoint = auth_map(&[("region", "europe-west1"), ("project_id", "my-project")]);
        let auth = auth_map(&[]);
        let result = GcpConnection::try_parse(&endpoint, "AUTOMATIC", &auth).unwrap();
        assert_eq!(result.region, "europe-west1");
        assert_eq!(result.project_id, "my-project");
        assert_eq!(result.authentication_mode, GcpAuthenticationMode::Automatic);
    }

    #[test]
    fn test_parse_gcp_connection_with_json_credentials() {
        let endpoint = auth_map(&[("region", "us-central1"), ("project_id", "test-project")]);
        let json = r#"{"type":"service_account","project_id":"test"}"#;
        let auth = auth_map(&[("content", json)]);
        let result = GcpConnection::try_parse(&endpoint, "JSON_CREDENTIALS", &auth).unwrap();
        assert_eq!(result.region, "us-central1");
        assert_eq!(result.project_id, "test-project");
        assert_eq!(
            result.authentication_mode,
            GcpAuthenticationMode::JsonCredentials {
                content: json.to_string()
            }
        );
    }

    #[test]
    fn test_parse_gcp_connection_missing_region() {
        let endpoint = auth_map(&[("project_id", "my-project")]);
        let auth = auth_map(&[]);
        let err = GcpConnection::try_parse(&endpoint, "AUTOMATIC", &auth).unwrap_err();
        assert!(err.message.contains("missing endpoint.region for GCP_SECRETS_MANAGER"));
    }

    #[test]
    fn test_parse_gcp_connection_missing_project_id() {
        let endpoint = auth_map(&[("region", "europe-west1")]);
        let auth = auth_map(&[]);
        let err = GcpConnection::try_parse(&endpoint, "AUTOMATIC", &auth).unwrap_err();
        assert!(
            err.message
                .contains("missing endpoint.project_id for GCP_SECRETS_MANAGER")
        );
    }

    #[test]
    fn test_parse_gcp_connection_unknown_auth_mode() {
        let endpoint = auth_map(&[("region", "europe-west1"), ("project_id", "my-project")]);
        let auth = auth_map(&[]);
        let err = GcpConnection::try_parse(&endpoint, "OAUTH2", &auth).unwrap_err();
        assert!(err.message.contains("unknown GCP authentication mode: OAUTH2"));
    }
}

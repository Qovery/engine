use super::SecretsManagerConversionError;
use std::collections::HashMap;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum AwsSecretsManagerSource {
    AwsSecretsManager,
    AwsParameterStore,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum AwsAuthenticationMode {
    Automatic,
    ArnRole {
        arn_role: String,
    },
    AwsStaticCredentials {
        access_key_id: String,
        secret_access_key: String,
    },
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum AwsAuthenticationModeKey {
    Automatic,
    ArnRole,
    StaticCreds,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AwsConnection {
    pub source: AwsSecretsManagerSource,
    pub region: String,
    pub authentication_mode: AwsAuthenticationMode,
}

impl TryFrom<&str> for AwsAuthenticationModeKey {
    type Error = SecretsManagerConversionError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "AUTOMATIC" => Ok(Self::Automatic),
            "ARN_ROLE" => Ok(Self::ArnRole),
            "STATIC_CREDS" => Ok(Self::StaticCreds),
            _ => Err(SecretsManagerConversionError {
                message: format!("unknown AWS authentication mode: {}", s),
            }),
        }
    }
}

impl AwsConnection {
    pub fn try_parse(
        endpoint_type: &str,
        endpoint: &HashMap<String, String>,
        auth_mode: &str,
        authentication: &HashMap<String, String>,
    ) -> Result<AwsConnection, SecretsManagerConversionError> {
        let region = endpoint
            .get("region")
            .ok_or_else(|| SecretsManagerConversionError {
                message: format!("missing endpoint.region for {}", endpoint_type),
            })?
            .clone();

        let authentication_mode = AwsAuthenticationMode::try_parse(auth_mode, authentication)?;

        let source = if endpoint_type == "AWS_SECRETS_MANAGER" {
            AwsSecretsManagerSource::AwsSecretsManager
        } else {
            AwsSecretsManagerSource::AwsParameterStore
        };

        Ok(AwsConnection {
            source,
            region,
            authentication_mode,
        })
    }
}

impl AwsAuthenticationMode {
    pub fn try_parse(
        mode: &str,
        auth: &HashMap<String, String>,
    ) -> Result<AwsAuthenticationMode, SecretsManagerConversionError> {
        let auth_mode = AwsAuthenticationModeKey::try_from(mode)?;
        match auth_mode {
            AwsAuthenticationModeKey::Automatic => Ok(AwsAuthenticationMode::Automatic),
            AwsAuthenticationModeKey::ArnRole => {
                let arn_role = auth
                    .get("arn_role")
                    .ok_or_else(|| SecretsManagerConversionError {
                        message: "missing authentication.arn_role for ARN_ROLE mode".to_string(),
                    })?
                    .clone();
                Ok(AwsAuthenticationMode::ArnRole { arn_role })
            }
            AwsAuthenticationModeKey::StaticCreds => {
                let access_key_id = auth
                    .get("access_key_id")
                    .ok_or_else(|| SecretsManagerConversionError {
                        message: "missing authentication.access_key_id for STATIC_CREDS mode".to_string(),
                    })?
                    .clone();
                let secret_access_key = auth
                    .get("secret_access_key")
                    .ok_or_else(|| SecretsManagerConversionError {
                        message: "missing authentication.secret_access_key for STATIC_CREDS mode".to_string(),
                    })?
                    .clone();
                Ok(AwsAuthenticationMode::AwsStaticCredentials {
                    access_key_id,
                    secret_access_key,
                })
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
            AwsAuthenticationModeKey::try_from("AUTOMATIC").unwrap(),
            AwsAuthenticationModeKey::Automatic
        );
        assert_eq!(
            AwsAuthenticationModeKey::try_from("ARN_ROLE").unwrap(),
            AwsAuthenticationModeKey::ArnRole
        );
        assert_eq!(
            AwsAuthenticationModeKey::try_from("STATIC_CREDS").unwrap(),
            AwsAuthenticationModeKey::StaticCreds
        );
    }

    #[test]
    fn test_authentication_mode_key_unknown_mode() {
        let err = AwsAuthenticationModeKey::try_from("KERBEROS").unwrap_err();
        assert!(err.message.contains("unknown AWS authentication mode: KERBEROS"));
    }

    #[test]
    fn test_parse_authentication_automatic() {
        let result = AwsAuthenticationMode::try_parse("AUTOMATIC", &auth_map(&[])).unwrap();
        assert_eq!(result, AwsAuthenticationMode::Automatic);
    }

    #[test]
    fn test_parse_authentication_arn_role() {
        let auth = auth_map(&[("arn_role", "arn:aws:iam::123456789012:role/my-role")]);
        let result = AwsAuthenticationMode::try_parse("ARN_ROLE", &auth).unwrap();
        assert_eq!(
            result,
            AwsAuthenticationMode::ArnRole {
                arn_role: "arn:aws:iam::123456789012:role/my-role".to_string()
            }
        );
    }

    #[test]
    fn test_parse_authentication_arn_role_missing_field() {
        let err = AwsAuthenticationMode::try_parse("ARN_ROLE", &auth_map(&[])).unwrap_err();
        assert!(err.message.contains("missing authentication.arn_role"));
    }

    #[test]
    fn test_parse_authentication_static_creds() {
        let auth = auth_map(&[
            ("access_key_id", "AKIAIOSFODNN7EXAMPLE"),
            ("secret_access_key", "SECRET_KEYEMI/K7MDENG/bPxRfiCY"),
        ]);
        let result = AwsAuthenticationMode::try_parse("STATIC_CREDS", &auth).unwrap();
        assert_eq!(
            result,
            AwsAuthenticationMode::AwsStaticCredentials {
                access_key_id: "AKIAIOSFODNN7EXAMPLE".to_string(),
                secret_access_key: "SECRET_KEYEMI/K7MDENG/bPxRfiCY".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_authentication_static_creds_missing_access_key_id() {
        let auth = auth_map(&[("secret_access_key", "SK")]);
        let err = AwsAuthenticationMode::try_parse("STATIC_CREDS", &auth).unwrap_err();
        assert!(err.message.contains("missing authentication.access_key_id"));
    }

    #[test]
    fn test_parse_authentication_static_creds_missing_secret_access_key() {
        let auth = auth_map(&[("access_key_id", "AK")]);
        let err = AwsAuthenticationMode::try_parse("STATIC_CREDS", &auth).unwrap_err();
        assert!(err.message.contains("missing authentication.secret_access_key"));
    }

    #[test]
    fn test_parse_authentication_unknown_mode() {
        let err = AwsAuthenticationMode::try_parse("OAUTH2", &auth_map(&[])).unwrap_err();
        assert!(err.message.contains("unknown AWS authentication mode: OAUTH2"));
    }

    #[test]
    fn test_aws_connection_secrets_manager_with_automatic() {
        let conn = AwsConnection {
            source: AwsSecretsManagerSource::AwsSecretsManager,
            region: "eu-west-3".to_string(),
            authentication_mode: AwsAuthenticationMode::Automatic,
        };
        assert_eq!(conn.source, AwsSecretsManagerSource::AwsSecretsManager);
        assert_eq!(conn.region, "eu-west-3");
        assert_eq!(conn.authentication_mode, AwsAuthenticationMode::Automatic);
    }

    #[test]
    fn test_aws_connection_parameter_store_with_arn_role() {
        let conn = AwsConnection {
            source: AwsSecretsManagerSource::AwsParameterStore,
            region: "us-east-1".to_string(),
            authentication_mode: AwsAuthenticationMode::ArnRole {
                arn_role: "arn:aws:iam::123:role/r".to_string(),
            },
        };
        assert_eq!(conn.source, AwsSecretsManagerSource::AwsParameterStore);
        assert_eq!(conn.region, "us-east-1");
        match conn.authentication_mode {
            AwsAuthenticationMode::ArnRole { arn_role } => assert_eq!(arn_role, "arn:aws:iam::123:role/r"),
            _ => panic!("Expected ArnRole"),
        }
    }

    #[test]
    fn test_aws_connection_equality() {
        let conn_a = AwsConnection {
            source: AwsSecretsManagerSource::AwsSecretsManager,
            region: "eu-west-3".to_string(),
            authentication_mode: AwsAuthenticationMode::Automatic,
        };
        let conn_b = conn_a.clone();
        assert_eq!(conn_a, conn_b);
    }

    #[test]
    fn test_parse_aws_connection_secrets_manager_with_automatic() {
        let endpoint = auth_map(&[("region", "eu-west-3")]);
        let auth = auth_map(&[]);
        let result = AwsConnection::try_parse("AWS_SECRETS_MANAGER", &endpoint, "AUTOMATIC", &auth).unwrap();
        assert_eq!(result.source, AwsSecretsManagerSource::AwsSecretsManager);
        assert_eq!(result.region, "eu-west-3");
        assert_eq!(result.authentication_mode, AwsAuthenticationMode::Automatic);
    }

    #[test]
    fn test_parse_aws_connection_parameter_store_with_automatic() {
        let endpoint = auth_map(&[("region", "us-east-1")]);
        let auth = auth_map(&[]);
        let result = AwsConnection::try_parse("AWS_PARAMETER_STORE", &endpoint, "AUTOMATIC", &auth).unwrap();
        assert_eq!(result.source, AwsSecretsManagerSource::AwsParameterStore);
        assert_eq!(result.region, "us-east-1");
    }

    #[test]
    fn test_parse_aws_connection_secrets_manager_with_arn_role() {
        let endpoint = auth_map(&[("region", "eu-west-3")]);
        let auth = auth_map(&[("arn_role", "arn:aws:iam::123456789012:role/my-role")]);
        let result = AwsConnection::try_parse("AWS_SECRETS_MANAGER", &endpoint, "ARN_ROLE", &auth).unwrap();
        assert_eq!(result.source, AwsSecretsManagerSource::AwsSecretsManager);
        assert_eq!(
            result.authentication_mode,
            AwsAuthenticationMode::ArnRole {
                arn_role: "arn:aws:iam::123456789012:role/my-role".to_string()
            }
        );
    }

    #[test]
    fn test_parse_aws_connection_parameter_store_with_static_creds() {
        let endpoint = auth_map(&[("region", "us-east-1")]);
        let auth = auth_map(&[
            ("access_key_id", "AKIAIOSFODNN7EXAMPLE"),
            ("secret_access_key", "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"),
        ]);
        let result = AwsConnection::try_parse("AWS_PARAMETER_STORE", &endpoint, "STATIC_CREDS", &auth).unwrap();
        assert_eq!(result.source, AwsSecretsManagerSource::AwsParameterStore);
        assert_eq!(
            result.authentication_mode,
            AwsAuthenticationMode::AwsStaticCredentials {
                access_key_id: "AKIAIOSFODNN7EXAMPLE".to_string(),
                secret_access_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_aws_connection_missing_region() {
        let endpoint = auth_map(&[]);
        let auth = auth_map(&[]);
        let err = AwsConnection::try_parse("AWS_SECRETS_MANAGER", &endpoint, "AUTOMATIC", &auth).unwrap_err();
        assert!(err.message.contains("missing endpoint.region for AWS_SECRETS_MANAGER"));
    }

    #[test]
    fn test_parse_aws_connection_missing_region_for_parameter_store() {
        let endpoint = auth_map(&[]);
        let auth = auth_map(&[]);
        let err = AwsConnection::try_parse("AWS_PARAMETER_STORE", &endpoint, "AUTOMATIC", &auth).unwrap_err();
        assert!(err.message.contains("missing endpoint.region for AWS_PARAMETER_STORE"));
    }

    #[test]
    fn test_parse_aws_connection_unknown_auth_mode() {
        let endpoint = auth_map(&[("region", "eu-west-3")]);
        let auth = auth_map(&[]);
        let err = AwsConnection::try_parse("AWS_SECRETS_MANAGER", &endpoint, "OAUTH2", &auth).unwrap_err();
        assert!(err.message.contains("unknown AWS authentication mode: OAUTH2"));
    }
}

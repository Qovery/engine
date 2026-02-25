pub mod aws_secrets_manager_authentication;
pub mod gcp_secrets_manager_authentication;

use crate::infrastructure::models::external_secrets::aws_secrets_manager_authentication::AwsConnection;
use crate::infrastructure::models::external_secrets::gcp_secrets_manager_authentication::GcpConnection;
use crate::io_models::eso::SecretsManagerAccessDto;
use std::fmt;

/// Domain layer
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SecretsManagerAccess {
    pub id: String,
    pub connection: SecretsManagerConnection,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum SecretsManagerConnection {
    Aws(AwsConnection),
    Gcp(GcpConnection),
}

#[derive(Debug, Eq, PartialEq)]
pub struct SecretsManagerConversionError {
    pub message: String,
}

impl fmt::Display for SecretsManagerConversionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl TryFrom<&SecretsManagerAccessDto> for SecretsManagerAccess {
    type Error = SecretsManagerConversionError;

    fn try_from(dto: &SecretsManagerAccessDto) -> Result<Self, Self::Error> {
        let endpoint_type = dto.endpoint.get("type").ok_or_else(|| SecretsManagerConversionError {
            message: "missing endpoint.type".to_string(),
        })?;

        let auth_mode = dto
            .authentication
            .get("mode")
            .ok_or_else(|| SecretsManagerConversionError {
                message: "missing authentication.mode".to_string(),
            })?;

        let connection = match endpoint_type.as_str() {
            "AWS_SECRETS_MANAGER" | "AWS_PARAMETER_STORE" => SecretsManagerConnection::Aws(AwsConnection::try_parse(
                endpoint_type,
                &dto.endpoint,
                auth_mode,
                &dto.authentication,
            )?),
            "GCP_SECRETS_MANAGER" => {
                SecretsManagerConnection::Gcp(GcpConnection::try_parse(&dto.endpoint, auth_mode, &dto.authentication)?)
            }
            _ => {
                return Err(SecretsManagerConversionError {
                    message: format!("unknown endpoint type: {}", endpoint_type),
                });
            }
        };

        Ok(SecretsManagerAccess {
            id: dto.id.clone(),
            connection,
        })
    }
}

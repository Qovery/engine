mod database;
pub mod io;
mod job;
mod router;
mod terraform_service;

use crate::constants::GCP_CLOUDSDK_CONFIG;
use crate::environment::models::types::{CloudProvider, GCP};
use crate::infrastructure::models::cloud_provider::Kind;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use thiserror::Error;
use url::Url;
use uuid::Uuid;

#[derive(Clone, Error, Debug, PartialEq, Eq)]
pub enum CredentialsError {
    #[error("Cannot create credentials: {raw_error_message:?}.")]
    CannotCreateCredentials { raw_error_message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonCredentialsType {
    ServiceAccount,
}

impl Display for JsonCredentialsType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            JsonCredentialsType::ServiceAccount => "service_account",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonCredentials {
    pub r#type: JsonCredentialsType,
    // Service account fields
    pub client_email: String,
    pub client_id: String,
    pub private_key_id: String,
    pub private_key: String,
    pub auth_uri: Url,
    pub token_uri: Url,
    pub auth_provider_x509_cert_url: Url,
    pub client_x509_cert_url: Url,
    pub project_id: String,
    pub universe_domain: String,
    pub cloudsdk_config_path: PathBuf,
    raw_json: String,
}

impl JsonCredentials {
    pub fn raw_json(&self) -> &str {
        &self.raw_json
    }

    pub fn cloudsdk_config(&self) -> (&str, &str) {
        (GCP_CLOUDSDK_CONFIG, self.cloudsdk_config_path.to_str().unwrap_or_default())
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct GcpAccessTokenCredentials {
    pub project_id: String,
    pub access_token: String,
    pub expiration_timestamp_ms: Option<i64>,
    pub cloudsdk_config_path: PathBuf,
}

impl std::fmt::Debug for GcpAccessTokenCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GcpAccessTokenCredentials")
            .field("project_id", &self.project_id)
            .field("access_token", &"****")
            .field("expiration_timestamp_ms", &self.expiration_timestamp_ms)
            .field("cloudsdk_config_path", &self.cloudsdk_config_path)
            .finish()
    }
}

impl GcpAccessTokenCredentials {
    pub fn new(project_id: String, access_token: String, expiration_timestamp_ms: Option<i64>) -> Self {
        Self {
            cloudsdk_config_path: PathBuf::from("/tmp/").join(format!("gcloud-{project_id}-{}", Uuid::new_v4())),
            project_id,
            access_token,
            expiration_timestamp_ms,
        }
    }

    pub fn cloudsdk_config(&self) -> (&str, &str) {
        (GCP_CLOUDSDK_CONFIG, self.cloudsdk_config_path.to_str().unwrap_or_default())
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum GcpCredentials {
    ServiceAccount(Box<JsonCredentials>),
    AccessToken(GcpAccessTokenCredentials),
}

impl std::fmt::Debug for GcpCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GcpCredentials::ServiceAccount(credentials) => f
                .debug_struct("GcpCredentials::ServiceAccount")
                .field("project_id", &credentials.project_id)
                .field("client_email", &credentials.client_email)
                .field("private_key", &"****")
                .finish(),
            GcpCredentials::AccessToken(credentials) => {
                f.debug_tuple("GcpCredentials::AccessToken").field(credentials).finish()
            }
        }
    }
}

impl GcpCredentials {
    pub fn project_id(&self) -> &str {
        match self {
            GcpCredentials::ServiceAccount(credentials) => credentials.project_id.as_str(),
            GcpCredentials::AccessToken(credentials) => credentials.project_id.as_str(),
        }
    }

    pub fn cloudsdk_config(&self) -> (&str, &str) {
        match self {
            GcpCredentials::ServiceAccount(credentials) => credentials.cloudsdk_config(),
            GcpCredentials::AccessToken(credentials) => credentials.cloudsdk_config(),
        }
    }

    pub fn raw_json(&self) -> &str {
        match self {
            GcpCredentials::ServiceAccount(credentials) => credentials.raw_json(),
            GcpCredentials::AccessToken(_) => "",
        }
    }

    pub fn access_token(&self) -> Option<&str> {
        match self {
            GcpCredentials::ServiceAccount(_) => None,
            GcpCredentials::AccessToken(credentials) => Some(credentials.access_token.as_str()),
        }
    }

    pub fn service_account(&self) -> Option<&JsonCredentials> {
        match self {
            GcpCredentials::ServiceAccount(credentials) => Some(credentials),
            GcpCredentials::AccessToken(_) => None,
        }
    }
}

impl From<JsonCredentials> for GcpCredentials {
    fn from(value: JsonCredentials) -> Self {
        GcpCredentials::ServiceAccount(Box::new(value))
    }
}

pub struct GcpAppExtraSettings {}
pub struct GcpDbExtraSettings {}
pub struct GcpRouterExtraSettings {}

impl CloudProvider for GCP {
    type AppExtraSettings = GcpAppExtraSettings;
    type DbExtraSettings = GcpDbExtraSettings;
    type RouterExtraSettings = GcpRouterExtraSettings;
    fn cloud_provider() -> Kind {
        Kind::Gcp
    }

    fn short_name() -> &'static str {
        "GCP"
    }

    fn full_name() -> &'static str {
        "Google"
    }

    fn registry_short_name() -> &'static str {
        "GCP AR"
    }

    fn registry_full_name() -> &'static str {
        "Google Artifact Registry"
    }

    fn lib_directory_name() -> &'static str {
        "gcp"
    }
}

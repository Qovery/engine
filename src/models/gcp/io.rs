use crate::models::gcp::{JsonCredentials as GkeJsonCredentials, JsonCredentialsType as GkeJsonCredentialsType};
use serde_derive::{Deserialize, Serialize};
use std::str::FromStr;
use url::Url;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum JsonCredentialsType {
    #[serde(rename = "service_account")]
    ServiceAccount,
}

impl From<JsonCredentialsType> for GkeJsonCredentialsType {
    fn from(value: JsonCredentialsType) -> Self {
        match value {
            JsonCredentialsType::ServiceAccount => GkeJsonCredentialsType::ServiceAccount,
        }
    }
}

impl From<GkeJsonCredentialsType> for JsonCredentialsType {
    fn from(value: GkeJsonCredentialsType) -> Self {
        match value {
            GkeJsonCredentialsType::ServiceAccount => JsonCredentialsType::ServiceAccount,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct JsonCredentials {
    pub r#type: JsonCredentialsType,
    // Service account fields
    pub client_email: String,
    pub client_id: String,
    pub private_key_id: String,
    pub private_key: String,
    pub auth_uri: String,
    pub token_uri: String,
    pub auth_provider_x509_cert_url: String,
    pub client_x509_cert_url: String,
    pub project_id: String,
    pub universe_domain: String,
}

impl TryFrom<JsonCredentials> for GkeJsonCredentials {
    type Error = String;

    fn try_from(value: JsonCredentials) -> Result<Self, Self::Error> {
        Ok(GkeJsonCredentials {
            r#type: GkeJsonCredentialsType::from(value.r#type),
            client_email: value.client_email,
            client_id: value.client_id,
            private_key: value.private_key,
            private_key_id: value.private_key_id,
            auth_uri: Url::from_str(&value.auth_uri).map_err(|_e| "Cannot parse auth_uri to URL")?,
            token_uri: Url::from_str(&value.token_uri).map_err(|_e| "Cannot parse token_uri to URL")?,
            auth_provider_x509_cert_url: Url::from_str(&value.auth_provider_x509_cert_url)
                .map_err(|_e| "Cannot parse auth_provider_x509_cert_url to URL")?,
            client_x509_cert_url: Url::from_str(&value.client_x509_cert_url)
                .map_err(|_e| "Cannot parse client_x509_cert_url to URL")?,
            project_id: value.project_id,
            universe_domain: value.universe_domain,
        })
    }
}

impl From<GkeJsonCredentials> for JsonCredentials {
    fn from(value: GkeJsonCredentials) -> Self {
        JsonCredentials {
            r#type: JsonCredentialsType::from(value.r#type),
            client_email: value.client_email,
            client_id: value.client_id,
            private_key: value.private_key,
            private_key_id: value.private_key_id,
            auth_uri: value.auth_uri.to_string(),
            token_uri: value.token_uri.to_string(),
            auth_provider_x509_cert_url: value.auth_provider_x509_cert_url.to_string(),
            client_x509_cert_url: value.client_x509_cert_url.to_string(),
            project_id: value.project_id,
            universe_domain: value.universe_domain,
        }
    }
}

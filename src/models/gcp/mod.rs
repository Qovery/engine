pub mod io;

use std::fmt::{Display, Formatter};
use thiserror::Error;
use url::Url;

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
}

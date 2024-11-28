mod database;
pub mod io;
mod job;
mod router;

use crate::cloud_provider::Kind;
use crate::models::types::{CloudProvider, GCP};
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

// https://cloud.google.com/kubernetes-engine/docs/how-to/persistent-volumes/gce-pd-csi-driver
#[derive(Clone, Eq, PartialEq)]
pub enum GcpStorageType {
    Ssd,
    Balanced,
}

impl GcpStorageType {
    pub fn to_k8s_storage_class(&self) -> String {
        match self {
            GcpStorageType::Ssd => "gcp-pd-ssd",
            GcpStorageType::Balanced => "gcp-pd-balanced",
        }
        .to_string()
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

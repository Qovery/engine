use crate::models::ToCloudProviderFormat;
use thiserror::Error;

#[derive(Clone, Error, Debug, PartialEq, Eq)]
pub enum CredentialsError {
    #[error("Cannot create credentials: {raw_error_message:?}.")]
    CannotCreateCredentials { raw_error_message: String },
}

pub struct Credentials {
    raw_content_json_string: String,
}

impl Credentials {
    pub fn new(raw_content_json_string: String) -> Self {
        Self {
            raw_content_json_string,
        }
    }
}

impl ToCloudProviderFormat for Credentials {
    fn to_cloud_provider_format(&self) -> String {
        self.raw_content_json_string.to_string()
    }
}

use rusoto_core::RusotoError;

use crate::cloud_provider::error::CloudProviderError::Error;

#[derive(Debug)]
pub enum CloudProviderError {
    Credentials,
    Error(Box<dyn std::error::Error>),
    Unknown,
}

impl From<Box<dyn std::error::Error>> for CloudProviderError {
    fn from(error: Box<dyn std::error::Error>) -> Self {
        Error(error)
    }
}

impl<E> From<RusotoError<E>> for CloudProviderError {
    fn from(error: RusotoError<E>) -> Self {
        match error {
            RusotoError::Credentials(_) => CloudProviderError::Credentials,
            RusotoError::Service(_) => CloudProviderError::Unknown,
            RusotoError::HttpDispatch(_) => CloudProviderError::Unknown,
            RusotoError::Validation(_) => CloudProviderError::Unknown,
            RusotoError::ParseError(_) => CloudProviderError::Unknown,
            RusotoError::Unknown(e) => {
                if e.status == 403 {
                    CloudProviderError::Credentials
                } else {
                    CloudProviderError::Unknown
                }
            }
            RusotoError::Blocking => CloudProviderError::Unknown,
        }
    }
}

#[derive(Debug)]
pub enum KubernetesError {}

#[derive(Debug)]
pub enum ServiceError {}

#[derive(Debug)]
pub enum DeployError {}

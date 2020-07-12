use crate::build_platform::Image;
use crate::cloud_provider::kubernetes::Kubernetes;
use rusoto_core::RusotoError;
use serde::{Deserialize, Serialize};
use std::any::Any;

pub mod application;
pub mod aws;
pub mod environment;
pub mod gcp;
pub mod kubernetes;
pub mod service;

pub trait CloudProvider {
    fn kind(&self) -> Kind;
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn is_valid(&self) -> Result<(), CloudProviderError>;
    fn kubernetes_clusters(self) -> Result<Vec<Box<dyn Kubernetes>>, CloudProviderError>;
    fn as_any(&self) -> &dyn Any;
}

#[derive(Debug)]
pub enum CloudProviderError {
    Credentials,
    Error(Box<dyn std::error::Error>),
    Unknown,
}

impl From<Box<dyn std::error::Error>> for CloudProviderError {
    fn from(error: Box<dyn std::error::Error>) -> Self {
        CloudProviderError::Error(error)
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
pub enum DeployError {
    Error,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum Kind {
    AWS,
    GCP,
}

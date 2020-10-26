use std::any::Any;
use std::rc::Rc;

use rusoto_core::RusotoError;
use serde::{Deserialize, Serialize};

use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::models::{Context, Listener, ProgressListener};

pub mod aws;
pub mod digitalocean;
pub mod environment;
pub mod gcp;
pub mod kubernetes;
pub mod service;

pub trait CloudProvider {
    fn context(&self) -> &Context;
    fn kind(&self) -> Kind;
    fn id(&self) -> &str;
    fn organization_id(&self) -> &str;
    fn name(&self) -> &str;
    fn is_valid(&self) -> Result<(), CloudProviderError>;
    fn add_listener(&mut self, listener: Listener);
    fn terraform_state_credentials(&self) -> &TerraformStateCredentials;
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
    DO,
}

pub struct TerraformStateCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub region: String,
}

pub enum DeploymentTarget<'a> {
    // ManagedService = Managed by the Cloud Provider (eg. RDS, DynamoDB...)
    ManagedServices(&'a dyn Kubernetes, &'a Environment),
    // SelfHosted = Kubernetes or anything else that implies management on our side
    SelfHosted(&'a dyn Kubernetes, &'a Environment),
}

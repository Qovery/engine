use std::any::Any;
use std::rc::Rc;

use rusoto_core::RusotoError;
use serde::{Deserialize, Serialize};

use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::error::{EngineError, EngineErrorCause, EngineErrorScope};
use crate::models::{Context, Listener, ProgressListener};

pub mod aws;
pub mod common;
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
    fn name_with_id(&self) -> String {
        format!("{} ({})", self.name(), self.id())
    }
    fn is_valid(&self) -> Result<(), EngineError>;
    fn add_listener(&mut self, listener: Listener);
    fn terraform_state_credentials(&self) -> &TerraformStateCredentials;
    fn engine_error_scope(&self) -> EngineErrorScope {
        EngineErrorScope::CloudProvider(self.id().to_string(), self.name().to_string())
    }
    fn engine_error(&self, cause: EngineErrorCause, message: String) -> EngineError {
        EngineError::new(
            cause,
            self.engine_error_scope(),
            self.context().execution_id(),
            Some(message),
        )
    }
    fn as_any(&self) -> &dyn Any;
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

impl TerraformStateCredentials {
    pub fn new(access_key_id: &str, secret_access_key: &str, region: &str) -> Self {
        TerraformStateCredentials {
            access_key_id: access_key_id.to_string(),
            secret_access_key: secret_access_key.to_string(),
            region: region.to_string(),
        }
    }
}

pub enum DeploymentTarget<'a> {
    // ManagedService = Managed by the Cloud Provider (eg. RDS, DynamoDB...)
    ManagedServices(&'a dyn Kubernetes, &'a Environment),
    // SelfHosted = Kubernetes or anything else that implies management on our side
    SelfHosted(&'a dyn Kubernetes, &'a Environment),
}

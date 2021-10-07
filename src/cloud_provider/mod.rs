use std::any::Any;

use serde::{Deserialize, Serialize};

use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::error::{EngineError, EngineErrorCause, EngineErrorScope};
use crate::models::{Context, Listen};

pub mod aws;
pub mod digitalocean;
pub mod environment;
pub mod helm;
pub mod kubernetes;
pub mod metrics;
pub mod models;
pub mod qovery;
pub mod scaleway;
pub mod service;
pub mod utilities;

pub trait CloudProvider: Listen {
    fn context(&self) -> &Context;
    fn kind(&self) -> Kind;
    fn id(&self) -> &str;
    fn organization_id(&self) -> &str;
    fn name(&self) -> &str;
    fn name_with_id(&self) -> String {
        format!("{} ({})", self.name(), self.id())
    }
    fn is_valid(&self) -> Result<(), EngineError>;
    /// environment variables containing credentials
    fn credentials_environment_variables(&self) -> Vec<(&str, &str)>;
    /// environment variables to inject to generate Terraform files from templates
    fn tera_context_environment_variables(&self) -> Vec<(&str, &str)>;
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

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Kind {
    Aws,
    Do,
    Scw,
}

impl Kind {
    pub fn name(&self) -> &str {
        match self {
            Kind::Aws => "AWS",
            Kind::Do => "Digital Ocean",
            Kind::Scw => "Scaleway",
        }
    }
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

pub struct DeploymentTarget<'a> {
    pub kubernetes: &'a dyn Kubernetes,
    pub environment: &'a Environment,
}

use std::any::Any;

use crate::cloud_provider::{CloudProvider, EngineError, Kind, TerraformStateCredentials};
use crate::constants::{SCALEWAY_ACCESS_KEY, SCALEWAY_SECRET_KEY};
use crate::error::EngineErrorCause;
use crate::models::{Context, Listen, Listener, Listeners};
use crate::runtime::block_on;

pub mod application;
pub mod router;

pub struct Scaleway {
    context: Context,
    id: String,
    organization_id: String,
    name: String,
    pub access_key: String,
    pub secret_key: String,
    terraform_state_credentials: TerraformStateCredentials,
    listeners: Listeners,
}

impl Scaleway {
    pub fn new(
        context: Context,
        id: &str,
        organization_id: &str,
        name: &str,
        access_key: &str,
        secret_key: &str,
        terraform_state_credentials: TerraformStateCredentials,
    ) -> Scaleway {
        Scaleway {
            context,
            id: id.to_string(),
            organization_id: organization_id.to_string(),
            name: name.to_string(),
            access_key: access_key.to_string(),
            secret_key: secret_key.to_string(),
            terraform_state_credentials,
            listeners: vec![],
        }
    }
}

impl CloudProvider for Scaleway {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::Scw
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn organization_id(&self) -> &str {
        self.organization_id.as_str()
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn is_valid(&self) -> Result<(), EngineError> {
        // TODO(benjaminch): To be implemented
        Ok(())
    }

    fn credentials_environment_variables(&self) -> Vec<(&str, &str)> {
        vec![
            (SCALEWAY_ACCESS_KEY, self.access_key.as_str()),
            (SCALEWAY_SECRET_KEY, self.secret_key.as_str()),
        ]
    }

    fn tera_context_environment_variables(&self) -> Vec<(&str, &str)> {
        vec![
            ("scaleway_access_key", self.access_key.as_str()),
            ("scaleway_secret_key", self.secret_key.as_str()),
        ]
    }

    fn terraform_state_credentials(&self) -> &TerraformStateCredentials {
        &self.terraform_state_credentials
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Listen for Scaleway {
    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }
}

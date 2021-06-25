use std::any::Any;

use crate::cloud_provider::{CloudProvider, EngineError, Kind, TerraformStateCredentials};
use crate::constants::{SCW_ACCESS_KEY_ID, SCW_SECRET_ACCESS_KEY};
use crate::error::EngineErrorCause;
use crate::models::{Context, Listen, Listener, Listeners};
use crate::runtime::block_on;

pub mod application;

pub struct Scaleway {
    context: Context,
    id: String,
    organization_id: String,
    name: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    terraform_state_credentials: TerraformStateCredentials,
    listeners: Listeners,
}

impl Scaleway {
    pub fn new(
        context: Context,
        id: &str,
        organization_id: &str,
        name: &str,
        access_key_id: &str,
        secret_access_key: &str,
        terraform_state_credentials: TerraformStateCredentials,
    ) -> Scaleway {
        Scaleway {
            context,
            id: id.to_string(),
            organization_id: organization_id.to_string(),
            name: name.to_string(),
            access_key_id: access_key_id.to_string(),
            secret_access_key: secret_access_key.to_string(),
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
        Kind::scw
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
        let client = StsClient::new_with_client(self.client(), Region::default());
        let s = block_on(client.get_caller_identity(GetCallerIdentityRequest::default()));

        match s {
            Ok(_x) => Ok(()),
            Err(_) => {
                return Err(self.engine_error(
                    EngineErrorCause::User(
                        "Your Scaleway account seems to be no longer valid (bad Credentials). \
                    Please contact your Organization administrator to fix or change the Credentials.",
                    ),
                    format!("failed to login to Scaleway {}", self.name_with_id()),
                ));
            }
        }
    }

    fn credentials_environment_variables(&self) -> Vec<(&str, &str)> {
        vec![
            (SCW_ACCESS_KEY_ID, self.access_key_id.as_str()),
            (SCW_SECRET_ACCESS_KEY, self.secret_access_key.as_str()),
        ]
    }

    fn tera_context_environment_variables(&self) -> Vec<(&str, &str)> {
        vec![
            ("scw_access_key", self.access_key_id.as_str()),
            ("scw_secret_key", self.secret_access_key.as_str()),
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

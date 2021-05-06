use std::any::Any;

use rusoto_core::{Client, HttpClient, Region};
use rusoto_credential::StaticProvider;
use rusoto_sts::{GetCallerIdentityRequest, Sts, StsClient};

use crate::cloud_provider::{CloudProvider, EngineError, Kind, TerraformStateCredentials};
use crate::constants::{AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY};
use crate::error::EngineErrorCause;
use crate::models::{Context, Listen, Listener, Listeners};
use crate::runtime::block_on;

pub mod application;
pub mod databases;
pub mod external_service;
pub mod kubernetes;
pub mod router;

pub struct AWS {
    context: Context,
    id: String,
    organization_id: String,
    name: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    terraform_state_credentials: TerraformStateCredentials,
    listeners: Listeners,
}

impl AWS {
    pub fn new(
        context: Context,
        id: &str,
        organization_id: &str,
        name: &str,
        access_key_id: &str,
        secret_access_key: &str,
        terraform_state_credentials: TerraformStateCredentials,
    ) -> Self {
        AWS {
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

    pub fn credentials(&self) -> StaticProvider {
        StaticProvider::new(
            self.access_key_id.to_string(),
            self.secret_access_key.to_string(),
            None,
            None,
        )
    }

    pub fn client(&self) -> Client {
        Client::new_with(self.credentials(), HttpClient::new().unwrap())
    }
}

impl CloudProvider for AWS {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::Aws
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
                        "Your AWS account seems to be no longer valid (bad Credentials). \
                    Please contact your Organization administrator to fix or change the Credentials.",
                    ),
                    format!("failed to login to AWS {}", self.name_with_id()),
                ));
            }
        }
    }

    fn credentials_environment_variables(&self) -> Vec<(&str, &str)> {
        vec![
            (AWS_ACCESS_KEY_ID, self.access_key_id.as_str()),
            (AWS_SECRET_ACCESS_KEY, self.secret_access_key.as_str()),
        ]
    }

    fn tera_context_environment_variables(&self) -> Vec<(&str, &str)> {
        vec![
            ("aws_access_key", self.access_key_id.as_str()),
            ("aws_secret_key", self.secret_access_key.as_str()),
        ]
    }

    fn terraform_state_credentials(&self) -> &TerraformStateCredentials {
        &self.terraform_state_credentials
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Listen for AWS {
    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }
}

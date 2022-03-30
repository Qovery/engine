use std::any::Any;

use rusoto_core::{Client, HttpClient, Region};
use rusoto_credential::StaticProvider;
use rusoto_sts::{GetCallerIdentityRequest, Sts, StsClient};
use uuid::Uuid;

use crate::cloud_provider::{CloudProvider, Kind, TerraformStateCredentials};
use crate::constants::{AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY};
use crate::errors::EngineError;
use crate::events::{EventDetails, GeneralStep, Stage, ToTransmitter, Transmitter};
use crate::io_models::{Context, Listen, Listener, Listeners, QoveryIdentifier};
use crate::runtime::block_on;

pub mod databases;
pub mod kubernetes;
pub mod regions;

pub struct AWS {
    context: Context,
    id: String,
    organization_id: String,
    organization_long_id: uuid::Uuid,
    name: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub zones: Vec<String>,
    terraform_state_credentials: TerraformStateCredentials,
    listeners: Listeners,
}

impl AWS {
    pub fn new(
        context: Context,
        id: &str,
        organization_id: &str,
        organization_long_id: uuid::Uuid,
        name: &str,
        access_key_id: &str,
        secret_access_key: &str,
        zones: Vec<String>,
        terraform_state_credentials: TerraformStateCredentials,
    ) -> Self {
        AWS {
            context,
            id: id.to_string(),
            organization_id: organization_id.to_string(),
            organization_long_id,
            name: name.to_string(),
            access_key_id: access_key_id.to_string(),
            secret_access_key: secret_access_key.to_string(),
            zones,
            terraform_state_credentials,
            listeners: vec![],
        }
    }

    pub fn credentials(&self) -> StaticProvider {
        StaticProvider::new(self.access_key_id.to_string(), self.secret_access_key.to_string(), None, None)
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

    fn organization_long_id(&self) -> Uuid {
        self.organization_long_id
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn access_key_id(&self) -> String {
        self.access_key_id.to_string()
    }

    fn secret_access_key(&self) -> String {
        self.secret_access_key.to_string()
    }

    fn token(&self) -> &str {
        todo!()
    }

    fn is_valid(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::General(GeneralStep::RetrieveClusterConfig));
        let client = StsClient::new_with_client(self.client(), Region::default());
        let s = block_on(client.get_caller_identity(GetCallerIdentityRequest::default()));

        match s {
            Ok(_x) => Ok(()),
            Err(_) => Err(EngineError::new_client_invalid_cloud_provider_credentials(event_details)),
        }
    }

    fn zones(&self) -> &Vec<String> {
        &self.zones
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

    fn get_event_details(&self, stage: Stage) -> EventDetails {
        let context = self.context();
        EventDetails::new(
            None,
            QoveryIdentifier::from(context.organization_id().to_string()),
            QoveryIdentifier::from(context.cluster_id().to_string()),
            QoveryIdentifier::from(context.execution_id().to_string()),
            None,
            stage,
            self.to_transmitter(),
        )
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

impl ToTransmitter for AWS {
    fn to_transmitter(&self) -> Transmitter {
        Transmitter::CloudProvider(self.id.to_string(), self.name.to_string())
    }
}

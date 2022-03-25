extern crate digitalocean;

use std::any::Any;

use digitalocean::DigitalOcean;
use uuid::Uuid;

use crate::cloud_provider::{CloudProvider, Kind, TerraformStateCredentials};
use crate::constants::DIGITAL_OCEAN_TOKEN;
use crate::errors::EngineError;
use crate::events::{EventDetails, GeneralStep, Stage, ToTransmitter, Transmitter};
use crate::models::{Context, Listen, Listener, Listeners, QoveryIdentifier};

pub mod application;
pub mod databases;
pub mod do_api_common;
pub mod kubernetes;
pub mod models;
pub mod network;
pub mod router;

pub struct DO {
    context: Context,
    id: String,
    organization_id: String,
    organization_long_id: uuid::Uuid,
    name: String,
    pub token: String,
    spaces_access_id: String,
    spaces_secret_key: String,
    terraform_state_credentials: TerraformStateCredentials,
    listeners: Listeners,
}

impl DO {
    pub fn new(
        context: Context,
        id: &str,
        organization_id: &str,
        organization_long_id: uuid::Uuid,
        token: &str,
        spaces_access_id: &str,
        spaces_secret_key: &str,
        name: &str,
        terraform_state_credentials: TerraformStateCredentials,
    ) -> Self {
        DO {
            context,
            id: id.to_string(),
            organization_id: organization_id.to_string(),
            organization_long_id,
            name: name.to_string(),
            token: token.to_string(),
            spaces_access_id: spaces_access_id.to_string(),
            spaces_secret_key: spaces_secret_key.to_string(),
            terraform_state_credentials,
            listeners: vec![],
        }
    }

    pub fn client(&self) -> DigitalOcean {
        DigitalOcean::new(self.token.as_str()).unwrap()
    }
}

impl CloudProvider for DO {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::Do
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
        self.spaces_access_id.to_string()
    }

    fn secret_access_key(&self) -> String {
        self.spaces_secret_key.to_string()
    }

    fn token(&self) -> &str {
        self.token.as_str()
    }

    fn is_valid(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::General(GeneralStep::RetrieveClusterConfig));
        let client = DigitalOcean::new(&self.token);
        match client {
            Ok(_x) => Ok(()),
            Err(_) => Err(EngineError::new_client_invalid_cloud_provider_credentials(event_details)),
        }
    }

    fn zones(&self) -> &Vec<String> {
        todo!()
    }

    fn credentials_environment_variables(&self) -> Vec<(&str, &str)> {
        vec![(DIGITAL_OCEAN_TOKEN, self.token.as_str())]
    }

    fn tera_context_environment_variables(&self) -> Vec<(&str, &str)> {
        vec![("digital_ocean_token", self.token.as_str())] // FIXME random key and value; is it good?
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

impl Listen for DO {
    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }
}

impl ToTransmitter for DO {
    fn to_transmitter(&self) -> Transmitter {
        Transmitter::CloudProvider(self.id.to_string(), self.name.to_string())
    }
}

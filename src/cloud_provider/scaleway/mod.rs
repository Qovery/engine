use std::any::Any;
use uuid::Uuid;

use crate::cloud_provider::{
    kubernetes::Kind as KubernetesKind, CloudProvider, EngineError, Kind, TerraformStateCredentials,
};
use crate::constants::{SCALEWAY_ACCESS_KEY, SCALEWAY_DEFAULT_PROJECT_ID, SCALEWAY_SECRET_KEY};
use crate::events::{EventDetails, Stage, Transmitter};
use crate::io_models::{Context, Listener, Listeners, QoveryIdentifier};

pub mod kubernetes;

pub struct Scaleway {
    context: Context,
    id: String,
    name: String,
    organization_id: String,
    organization_long_id: Uuid,
    access_key: String,
    secret_key: String,
    project_id: String,
    terraform_state_credentials: TerraformStateCredentials,
    listeners: Listeners,
}

impl Scaleway {
    pub fn new(
        context: Context,
        id: &str,
        organization_id: &str,
        organization_long_id: Uuid,
        name: &str,
        access_key: &str,
        secret_key: &str,
        project_id: &str,
        terraform_state_credentials: TerraformStateCredentials,
    ) -> Scaleway {
        Scaleway {
            context,
            id: id.to_string(),
            organization_id: organization_id.to_string(),
            organization_long_id,
            name: name.to_string(),
            access_key: access_key.to_string(),
            secret_key: secret_key.to_string(),
            project_id: project_id.to_string(),
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

    fn kubernetes_kind(&self) -> KubernetesKind {
        KubernetesKind::ScwKapsule
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
        self.access_key.to_string()
    }

    fn secret_access_key(&self) -> String {
        self.secret_key.to_string()
    }

    fn token(&self) -> &str {
        todo!()
    }

    fn is_valid(&self) -> Result<(), EngineError> {
        // TODO(benjaminch): To be implemented
        Ok(())
    }

    fn zones(&self) -> &Vec<String> {
        todo!()
    }

    fn credentials_environment_variables(&self) -> Vec<(&str, &str)> {
        vec![
            (SCALEWAY_ACCESS_KEY, self.access_key.as_str()),
            (SCALEWAY_SECRET_KEY, self.secret_key.as_str()),
            (SCALEWAY_DEFAULT_PROJECT_ID, self.project_id.as_str()),
        ]
    }

    fn tera_context_environment_variables(&self) -> Vec<(&str, &str)> {
        vec![
            ("scaleway_access_key", self.access_key.as_str()),
            ("scaleway_secret_key", self.secret_key.as_str()),
            ("scaleway_project_id", self.project_id.as_str()),
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

    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }

    fn to_transmitter(&self) -> Transmitter {
        Transmitter::CloudProvider(self.id.to_string(), self.name.to_string())
    }
}

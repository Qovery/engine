use std::any::Any;
use uuid::Uuid;

use crate::cloud_provider::{
    kubernetes::Kind as KubernetesKind, CloudProvider, EngineError, Kind, TerraformStateCredentials,
};
use crate::constants::{SCW_ACCESS_KEY, SCW_DEFAULT_PROJECT_ID, SCW_SECRET_KEY};
use crate::events::{EventDetails, Stage, Transmitter};
use crate::io_models::context::Context;
use crate::io_models::QoveryIdentifier;
use crate::utilities::to_short_id;

pub mod database_instance_type;
pub mod kubernetes;

pub struct Scaleway {
    context: Context,
    id: String,
    long_id: Uuid,
    name: String,
    access_key: String,
    secret_key: String,
    project_id: String,
    region: String,
    terraform_state_credentials: TerraformStateCredentials,
}

impl Scaleway {
    pub fn new(
        context: Context,
        long_id: Uuid,
        name: &str,
        access_key: &str,
        secret_key: &str,
        project_id: &str,
        region: &str,
        terraform_state_credentials: TerraformStateCredentials,
    ) -> Scaleway {
        Scaleway {
            context,
            id: to_short_id(&long_id),
            long_id,
            name: name.to_string(),
            access_key: access_key.to_string(),
            secret_key: secret_key.to_string(),
            project_id: project_id.to_string(),
            region: region.to_string(),
            terraform_state_credentials,
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
        self.context.organization_short_id()
    }

    fn organization_long_id(&self) -> Uuid {
        *self.context.organization_long_id()
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

    fn region(&self) -> String {
        self.region.clone()
    }

    fn aws_sdk_client(&self) -> Option<aws_config::SdkConfig> {
        None
    }

    fn token(&self) -> &str {
        todo!()
    }

    fn is_valid(&self) -> Result<(), Box<EngineError>> {
        // TODO(benjaminch): To be implemented
        Ok(())
    }

    fn zones(&self) -> &Vec<String> {
        todo!()
    }

    fn credentials_environment_variables(&self) -> Vec<(&str, &str)> {
        vec![
            (SCW_ACCESS_KEY, self.access_key.as_str()),
            (SCW_SECRET_KEY, self.secret_key.as_str()),
            (SCW_DEFAULT_PROJECT_ID, self.project_id.as_str()),
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
            QoveryIdentifier::new(*context.organization_long_id()),
            QoveryIdentifier::new(*context.cluster_long_id()),
            context.execution_id().to_string(),
            stage,
            self.to_transmitter(),
        )
    }

    fn to_transmitter(&self) -> Transmitter {
        Transmitter::CloudProvider(self.long_id, self.name.to_string())
    }
}

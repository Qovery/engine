pub mod chart_gen;
use uuid::Uuid;

use crate::io_models::context::Context;
pub mod kubernetes;
use crate::cloud_provider::{
    kubernetes::Kind as KubernetesKind, CloudProvider, EngineError, Kind, TerraformStateCredentials,
};
use crate::events::{EventDetails, Transmitter};
use crate::io_models::QoveryIdentifier;

pub struct SelfManaged {
    context: Context,
    long_id: String,
    name: String,
    region: String,
}

impl SelfManaged {
    pub fn new(context: Context, long_id: Uuid, name: String, region: String) -> Self {
        let long_id_string = long_id.to_string();
        SelfManaged {
            context,
            long_id: long_id_string,
            name,
            region,
        }
    }
}

impl CloudProvider for SelfManaged {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::SelfManaged
    }

    fn kubernetes_kind(&self) -> KubernetesKind {
        KubernetesKind::EksSelfManaged
    }

    fn id(&self) -> &str {
        self.long_id.as_str()
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn region(&self) -> String {
        self.region.clone()
    }

    fn organization_id(&self) -> &str {
        self.context.organization_short_id()
    }

    fn organization_long_id(&self) -> uuid::Uuid {
        *self.context.organization_long_id()
    }

    fn access_key_id(&self) -> String {
        "".to_string()
    }

    fn secret_access_key(&self) -> String {
        "".to_string()
    }

    fn aws_sdk_client(&self) -> Option<aws_types::SdkConfig> {
        None
    }

    fn is_valid(&self) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn zones(&self) -> Vec<String> {
        Vec::new()
    }

    fn credentials_environment_variables(&self) -> Vec<(&str, &str)> {
        Vec::new()
    }

    fn tera_context_environment_variables(&self) -> Vec<(&str, &str)> {
        Vec::new()
    }

    fn terraform_state_credentials(&self) -> Option<&TerraformStateCredentials> {
        None
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn get_event_details(&self, stage: crate::events::Stage) -> crate::events::EventDetails {
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

    fn to_transmitter(&self) -> crate::events::Transmitter {
        let uuid = Uuid::parse_str(self.long_id.as_str()).expect("Failed to parse UUID");
        Transmitter::CloudProvider(uuid, self.name.to_string())
    }
}

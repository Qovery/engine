pub mod kubernetes;
pub mod locations;

use crate::cloud_provider::gcp::locations::GcpRegion;
use crate::cloud_provider::{kubernetes::Kind as KubernetesKind, CloudProvider, Kind, TerraformStateCredentials};
use crate::constants::{GCP_CREDENTIALS, GCP_PROJECT, GCP_REGION};
use crate::errors::EngineError;
use crate::events::{EventDetails, Stage, Transmitter};
use crate::io_models::context::Context;
use crate::io_models::QoveryIdentifier;
use crate::models::gcp::io::JsonCredentials as JsonCredentialsIo;
use crate::models::gcp::JsonCredentials;
use crate::models::ToCloudProviderFormat;
use crate::utilities::to_short_id;
use std::any::Any;
use uuid::Uuid;

pub struct Google {
    context: Context,
    id: String,
    long_id: Uuid,
    name: String,
    json_credentials: JsonCredentials,
    json_credentials_raw_json: String,
    region: GcpRegion,
    terraform_state_credentials: TerraformStateCredentials,
}

impl Google {
    pub fn new(
        context: Context,
        long_id: Uuid,
        name: &str,
        json_credentials: JsonCredentials,
        region: GcpRegion,
        terraform_state_credentials: TerraformStateCredentials,
    ) -> Google {
        let credentials_io = JsonCredentialsIo::from(json_credentials.clone());

        Google {
            context,
            id: to_short_id(&long_id),
            long_id,
            name: name.to_string(),
            json_credentials,
            json_credentials_raw_json: serde_json::to_string(&credentials_io).unwrap_or_default(),
            region,
            terraform_state_credentials,
        }
    }
}

impl CloudProvider for Google {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::Gcp
    }

    fn kubernetes_kind(&self) -> KubernetesKind {
        KubernetesKind::Gke
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

    /// GKE access key is empty, credentials are in secret_access_key
    fn access_key_id(&self) -> String {
        "".to_string() // TODO(benjaminch): GKE integration to be checked but shouldn't be needed
    }

    fn secret_access_key(&self) -> String {
        // credentials JSON string to be returned as secret access key
        self.json_credentials_raw_json.to_string()
    }

    fn region(&self) -> String {
        self.region.to_cloud_provider_format().to_string()
    }

    fn aws_sdk_client(&self) -> Option<aws_config::SdkConfig> {
        None
    }

    fn is_valid(&self) -> Result<(), Box<EngineError>> {
        // TODO(benjaminch): To be implemented
        Ok(())
    }

    fn zones(&self) -> Vec<String> {
        self.region
            .zones()
            .iter()
            .map(|z| z.to_cloud_provider_format().to_string())
            .collect()
    }

    fn credentials_environment_variables(&self) -> Vec<(&str, &str)> {
        vec![
            (GCP_CREDENTIALS, self.json_credentials_raw_json.as_str()),
            (GCP_PROJECT, self.json_credentials.project_id.as_str()),
            (GCP_REGION, self.region.to_cloud_provider_format()),
        ]
    }

    fn tera_context_environment_variables(&self) -> Vec<(&str, &str)> {
        vec![
            ("gcp_json_credentials", self.json_credentials_raw_json.as_str()),
            ("gcp_project_id", self.json_credentials.project_id.as_str()),
            ("gcp_region", self.region.to_cloud_provider_format()),
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

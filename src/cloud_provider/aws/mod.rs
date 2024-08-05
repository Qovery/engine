use aws_config::provider_config::ProviderConfig;
use aws_types::SdkConfig;
use std::any::Any;

use aws_smithy_async::rt::sleep::TokioSleep;
use aws_smithy_client::erase::DynConnector;
use aws_smithy_client::never::NeverConnector;
use aws_types::os_shim_internal::Env;
use rusoto_core::{Client, HttpClient, Region};
use rusoto_credential::StaticProvider;
use rusoto_sts::{GetCallerIdentityRequest, Sts, StsClient};
use uuid::Uuid;

use crate::cloud_provider::{kubernetes::Kind as KubernetesKind, CloudProvider, Kind, TerraformStateCredentials};
use crate::constants::{AWS_ACCESS_KEY_ID, AWS_DEFAULT_REGION, AWS_SECRET_ACCESS_KEY};
use crate::errors::EngineError;
use crate::events::{EventDetails, InfrastructureStep, Stage, Transmitter};
use crate::io_models::context::Context;
use crate::io_models::QoveryIdentifier;
use crate::runtime::block_on;
use crate::utilities::to_short_id;

pub mod database_instance_type;
pub mod kubernetes;
pub mod regions;

pub struct AWS {
    context: Context,
    id: String,
    long_id: Uuid,
    name: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub region: String,
    pub zones: Vec<String>,
    kubernetes_kind: KubernetesKind,
    terraform_state_credentials: TerraformStateCredentials,
}

impl AWS {
    pub fn new(
        context: Context,
        long_id: Uuid,
        name: &str,
        access_key_id: &str,
        secret_access_key: &str,
        region: &str,
        zones: Vec<String>,
        kubernetes_kind: KubernetesKind,
        terraform_state_credentials: TerraformStateCredentials,
    ) -> Self {
        AWS {
            context,
            id: to_short_id(&long_id),
            long_id,
            name: name.to_string(),
            access_key_id: access_key_id.to_string(),
            secret_access_key: secret_access_key.to_string(),
            region: region.to_string(),
            zones,
            kubernetes_kind,
            terraform_state_credentials,
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

    fn kubernetes_kind(&self) -> KubernetesKind {
        self.kubernetes_kind
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
        self.access_key_id.to_string()
    }

    fn secret_access_key(&self) -> String {
        self.secret_access_key.to_string()
    }

    fn region(&self) -> String {
        self.region.to_string()
    }

    fn aws_sdk_client(&self) -> Option<SdkConfig> {
        let env = Env::from_slice(&[
            ("AWS_MAX_ATTEMPTS", "10"),
            ("AWS_REGION", self.region().as_str()),
            ("AWS_ACCESS_KEY_ID", self.access_key_id().as_str()),
            ("AWS_SECRET_ACCESS_KEY", self.secret_access_key().as_str()),
        ]);

        Some(block_on(
            aws_config::from_env()
                .configure(
                    ProviderConfig::empty()
                        .with_env(env)
                        .with_sleep(TokioSleep::new())
                        .with_http_connector(DynConnector::new(NeverConnector::new())),
                )
                .load(),
        ))
    }

    fn is_valid(&self) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::RetrieveClusterConfig));
        let client = StsClient::new_with_client(self.client(), Region::default());
        let s = block_on(client.get_caller_identity(GetCallerIdentityRequest::default()));

        match s {
            Ok(_x) => Ok(()),
            Err(_) => Err(Box::new(EngineError::new_client_invalid_cloud_provider_credentials(
                event_details,
            ))),
        }
    }

    fn zones(&self) -> Vec<String> {
        self.zones.clone()
    }

    fn credentials_environment_variables(&self) -> Vec<(&str, &str)> {
        vec![
            (AWS_DEFAULT_REGION, self.region.as_str()),
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

    fn terraform_state_credentials(&self) -> Option<&TerraformStateCredentials> {
        Some(&self.terraform_state_credentials)
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

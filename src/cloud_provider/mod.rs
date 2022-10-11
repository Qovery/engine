use std::any::Any;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

use aws_config::SdkConfig;
use serde::{Deserialize, Serialize};

use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cmd::docker::Docker;
use crate::cmd::helm::{to_engine_error, Helm};
use crate::container_registry::ContainerRegistry;
use crate::dns_provider::DnsProvider;
use crate::engine::EngineConfig;
use crate::errors::EngineError;
use crate::events::{EventDetails, Stage, Transmitter};
use crate::io_models::context::Context;
use crate::runtime::block_on;
use crate::utilities::create_kube_client;

pub mod aws;
pub mod digitalocean;
pub mod environment;
pub mod helm;
pub mod helm_charts;
pub mod io;
pub mod kubernetes;
pub mod metrics;
pub mod models;
pub mod qovery;
pub mod scaleway;
pub mod service;
pub mod utilities;

pub trait CloudProvider {
    fn context(&self) -> &Context;
    fn kind(&self) -> Kind;
    fn kubernetes_kind(&self) -> kubernetes::Kind;
    fn id(&self) -> &str;
    fn organization_id(&self) -> &str;
    fn organization_long_id(&self) -> uuid::Uuid;
    fn name(&self) -> &str;
    fn name_with_id(&self) -> String {
        format!("{} ({})", self.name(), self.id())
    }
    fn access_key_id(&self) -> String;
    fn secret_access_key(&self) -> String;
    fn region(&self) -> String;
    fn aws_sdk_client(&self) -> Option<SdkConfig>;
    fn token(&self) -> &str;
    fn is_valid(&self) -> Result<(), EngineError>;
    fn zones(&self) -> &Vec<String>;
    /// environment variables containing credentials
    fn credentials_environment_variables(&self) -> Vec<(&str, &str)>;
    /// environment variables to inject to generate Terraform files from templates
    fn tera_context_environment_variables(&self) -> Vec<(&str, &str)>;
    fn terraform_state_credentials(&self) -> &TerraformStateCredentials;
    fn as_any(&self) -> &dyn Any;
    fn get_event_details(&self, stage: Stage) -> EventDetails;
    fn to_transmitter(&self) -> Transmitter;
}

#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Kind {
    Aws,
    Do,
    Scw,
}

impl FromStr for Kind {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "aws" | "amazon" => Ok(Kind::Aws),
            "do" | "digitalocean" => Ok(Kind::Do),
            "scw" | "scaleway" => Ok(Kind::Scw),
            _ => Err(()),
        }
    }
}

impl Display for Kind {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Kind::Aws => "AWS",
            Kind::Do => "Digital Ocean",
            Kind::Scw => "Scaleway",
        })
    }
}

pub trait CloudProviderZones {}

pub struct TerraformStateCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub region: String,
}

impl TerraformStateCredentials {
    pub fn new(access_key_id: &str, secret_access_key: &str, region: &str) -> Self {
        TerraformStateCredentials {
            access_key_id: access_key_id.to_string(),
            secret_access_key: secret_access_key.to_string(),
            region: region.to_string(),
        }
    }
}

pub struct DeploymentTarget<'a> {
    pub kubernetes: &'a dyn Kubernetes,
    pub container_registry: &'a dyn ContainerRegistry,
    pub cloud_provider: &'a dyn CloudProvider,
    pub dns_provider: &'a dyn DnsProvider,
    pub environment: &'a Environment,
    pub docker: &'a Docker,
    pub kube: kube::Client,
    pub helm: Helm,
    pub should_abort: &'a dyn Fn() -> bool,
}

impl<'a> DeploymentTarget<'a> {
    pub fn new(
        engine_config: &'a EngineConfig,
        environment: &'a Environment,
        event_details: &EventDetails,
        should_abort: &'a dyn Fn() -> bool,
    ) -> Result<DeploymentTarget<'a>, EngineError> {
        let kubernetes = engine_config.kubernetes();
        let kubeconfig_path = kubernetes.get_kubeconfig_file_path().unwrap_or_default();
        let kube_credentials: Vec<(String, String)> = kubernetes
            .cloud_provider()
            .credentials_environment_variables()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        let kube_client = block_on(create_kube_client(kubeconfig_path.clone(), kube_credentials.as_slice()))
            .map_err(|err| EngineError::new_cannot_connect_to_k8s_cluster(event_details.clone(), err))?;

        let helm = Helm::new(
            &kubeconfig_path,
            &kubernetes.cloud_provider().credentials_environment_variables(),
        )
        .map_err(|e| to_engine_error(event_details, e))?;

        Ok(DeploymentTarget {
            kubernetes,
            container_registry: engine_config.container_registry(),
            cloud_provider: engine_config.cloud_provider(),
            dns_provider: engine_config.dns_provider(),
            environment,
            docker: &engine_config.context().docker,
            kube: kube_client,
            helm,
            should_abort,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::Kind;

    #[test]
    fn test_provider_kind_from_str() {
        // setup:
        let test_cases = vec![
            ("", Err(())),
            (" ", Err(())),
            ("aws", Ok(Kind::Aws)),
            ("amazon", Ok(Kind::Aws)),
            (" aws ", Ok(Kind::Aws)),
            (" amazon ", Ok(Kind::Aws)),
            ("AWS ", Ok(Kind::Aws)),
            ("amaZon", Ok(Kind::Aws)),
            ("amazon_blabla", Err(())),
            ("do", Ok(Kind::Do)),
            ("digitalocean", Ok(Kind::Do)),
            (" do ", Ok(Kind::Do)),
            (" digitalocean ", Ok(Kind::Do)),
            ("DO ", Ok(Kind::Do)),
            ("Do", Ok(Kind::Do)),
            ("do_blabla", Err(())),
            ("scw", Ok(Kind::Scw)),
            ("scaleway", Ok(Kind::Scw)),
            (" scw ", Ok(Kind::Scw)),
            (" scaleway ", Ok(Kind::Scw)),
            ("SCW ", Ok(Kind::Scw)),
            ("Scw", Ok(Kind::Scw)),
            ("scw_blabla", Err(())),
        ];

        for tc in test_cases {
            // execute:
            let result: Result<Kind, ()> = tc.0.parse();

            // verify:
            assert_eq!(tc.1, result);
        }
    }
}

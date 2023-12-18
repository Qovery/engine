use std::any::Any;
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use std::sync::Arc;

use aws_types::SdkConfig;
use serde::{Deserialize, Serialize};

use crate::cloud_provider::environment::Environment;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::service::Service;
use crate::cmd::docker::Docker;
use crate::cmd::helm::{to_engine_error, Helm};
use crate::container_registry::ContainerRegistry;
use crate::deployment_report::logger::EnvLogger;
use crate::deployment_report::obfuscation_service::ObfuscationService;
use crate::dns_provider::DnsProvider;
use crate::engine::InfrastructureContext;
use crate::errors::EngineError;
use crate::events::{EnvironmentStep, EventDetails, Stage, Transmitter};
use crate::io_models::context::Context;
use crate::logger::Logger;
use crate::metrics_registry::MetricsRegistry;
use crate::runtime::block_on;
use crate::utilities::create_kube_client;

pub mod aws;
pub mod environment;
pub mod gcp;
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
pub mod vault;

pub trait CloudProvider: Send + Sync {
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
    // TODO(benjaminch): Remove client from here
    fn aws_sdk_client(&self) -> Option<SdkConfig>;
    fn is_valid(&self) -> Result<(), Box<EngineError>>;
    fn zones(&self) -> Vec<String>;
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
    Scw,
    Gcp,
}

impl FromStr for Kind {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "aws" | "amazon" => Ok(Kind::Aws),
            "scw" | "scaleway" => Ok(Kind::Scw),
            "gcp" | "google" => Ok(Kind::Gcp),
            _ => Err(()),
        }
    }
}

impl Display for Kind {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Kind::Aws => "AWS",
            Kind::Scw => "Scaleway",
            Kind::Gcp => "Google",
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
    pub obfuscation_service: Box<dyn ObfuscationService>,
    pub cloud_provider: &'a dyn CloudProvider,
    pub dns_provider: &'a dyn DnsProvider,
    pub environment: &'a Environment,
    pub docker: &'a Docker,
    pub kube: kube::Client,
    pub helm: Helm,
    pub should_abort: &'a (dyn Fn() -> bool + Send + Sync),
    logger: Arc<Box<dyn Logger>>,
    pub metrics_registry: Arc<dyn MetricsRegistry>,
    pub is_dry_run_deploy: bool,
    pub is_test_cluster: bool,
}

impl<'a> DeploymentTarget<'a> {
    pub fn new(
        infra_ctx: &'a InfrastructureContext,
        environment: &'a Environment,
        obfuscation_service: Box<dyn ObfuscationService>,
        should_abort: &'a (dyn Fn() -> bool + Sync + Send),
    ) -> Result<DeploymentTarget<'a>, Box<EngineError>> {
        let event_details = environment.event_details();
        let kubernetes = infra_ctx.kubernetes();
        let kubeconfig_path = kubernetes.get_kubeconfig_file_path()?;
        let kubeconfig_path_str = kubeconfig_path.to_str().unwrap_or_default();
        let kube_credentials: Vec<(String, String)> = kubernetes
            .cloud_provider()
            .credentials_environment_variables()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        let kube_client = block_on(create_kube_client(kubeconfig_path_str, kube_credentials.as_slice()))
            .map_err(|err| EngineError::new_cannot_connect_to_k8s_cluster(event_details.clone(), err))?;

        let helm = Helm::new(
            kubeconfig_path_str,
            &kubernetes.cloud_provider().credentials_environment_variables(),
        )
        .map_err(|e| to_engine_error(event_details, e))?;

        Ok(DeploymentTarget {
            kubernetes,
            container_registry: infra_ctx.container_registry(),
            cloud_provider: infra_ctx.cloud_provider(),
            dns_provider: infra_ctx.dns_provider(),
            environment,
            docker: &infra_ctx.context().docker,
            kube: kube_client,
            helm,
            should_abort,
            logger: Arc::new(infra_ctx.kubernetes().logger().clone_dyn()),
            is_dry_run_deploy: kubernetes.context().is_dry_run_deploy(),
            is_test_cluster: kubernetes.context().is_test_cluster(),
            metrics_registry: Arc::from(infra_ctx.kubernetes().metrics_registry().clone_dyn()),
            obfuscation_service,
        })
    }

    pub fn env_logger(&self, service: &impl Service, step: EnvironmentStep) -> EnvLogger {
        EnvLogger::new(service, step, self.logger.clone())
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
            ("scw", Ok(Kind::Scw)),
            ("scaleway", Ok(Kind::Scw)),
            (" scw ", Ok(Kind::Scw)),
            (" scaleway ", Ok(Kind::Scw)),
            ("SCW ", Ok(Kind::Scw)),
            ("Scw", Ok(Kind::Scw)),
            ("scw_blabla", Err(())),
            ("gcp", Ok(Kind::Gcp)),
            ("google", Ok(Kind::Gcp)),
            (" gcp ", Ok(Kind::Gcp)),
            (" google ", Ok(Kind::Gcp)),
            ("GCP ", Ok(Kind::Gcp)),
            ("Gcp", Ok(Kind::Gcp)),
            ("gcp_blabla", Err(())),
        ];

        for tc in test_cases {
            // execute:
            let result: Result<Kind, ()> = tc.0.parse();

            // verify:
            assert_eq!(tc.1, result);
        }
    }
}

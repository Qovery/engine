use std::borrow::Borrow;
use std::sync::Mutex;
use thiserror::Error;

use crate::errors::EngineError;
use crate::events::InfrastructureStep;
use crate::events::Stage::Infrastructure;
use crate::infrastructure::models::build_platform::BuildPlatform;
use crate::infrastructure::models::cloud_provider::CloudProvider;
use crate::infrastructure::models::container_registry::ContainerRegistry;
use crate::infrastructure::models::dns_provider::DnsProvider;
use crate::infrastructure::models::kubernetes::Kubernetes;
use crate::io_models::context::Context;
use crate::metrics_registry::MetricsRegistry;
use crate::services::kube_client::QubeClient;

#[derive(Error, Debug, PartialEq, Eq)]
pub enum EngineConfigError {
    #[error("Build platform is not valid error: {0}")]
    BuildPlatformNotValid(EngineError),
    #[error("Cloud provider is not valid error: {0}")]
    CloudProviderNotValid(EngineError),
    #[error("DNS provider is not valid error: {0}")]
    DnsProviderNotValid(EngineError),
    #[error("Kubernetes is not valid error: {0}")]
    KubernetesNotValid(EngineError),
}

impl EngineConfigError {
    pub fn engine_error(&self) -> &EngineError {
        match self {
            EngineConfigError::BuildPlatformNotValid(e) => e,
            EngineConfigError::CloudProviderNotValid(e) => e,
            EngineConfigError::DnsProviderNotValid(e) => e,
            EngineConfigError::KubernetesNotValid(e) => e,
        }
    }
}

pub struct InfrastructureContext {
    context: Context,
    build_platform: Box<dyn BuildPlatform>,
    container_registry: Box<dyn ContainerRegistry>,
    cloud_provider: Box<dyn CloudProvider>,
    dns_provider: Box<dyn DnsProvider>,
    kubernetes: Box<dyn Kubernetes>,
    metrics_registry: Box<dyn MetricsRegistry>,
    is_infra_deployment: bool,
    kube_client: Mutex<Option<QubeClient>>,
}

impl InfrastructureContext {
    pub fn new(
        context: Context,
        build_platform: Box<dyn BuildPlatform>,
        container_registry: Box<dyn ContainerRegistry>,
        cloud_provider: Box<dyn CloudProvider>,
        dns_provider: Box<dyn DnsProvider>,
        kubernetes: Box<dyn Kubernetes>,
        metrics_registry: Box<dyn MetricsRegistry>,
        is_infra_deployment: bool,
    ) -> InfrastructureContext {
        InfrastructureContext {
            context,
            build_platform,
            container_registry,
            cloud_provider,
            dns_provider,
            kubernetes,
            metrics_registry,
            is_infra_deployment,
            kube_client: Mutex::new(None),
        }
    }

    pub fn kubernetes(&self) -> &dyn Kubernetes {
        self.kubernetes.as_ref()
    }

    pub fn context(&self) -> &Context {
        &self.context
    }

    pub fn context_mut(&mut self) -> &mut Context {
        &mut self.context
    }

    pub fn build_platform(&self) -> &dyn BuildPlatform {
        self.build_platform.borrow()
    }

    pub fn container_registry(&self) -> &dyn ContainerRegistry {
        self.container_registry.borrow()
    }

    pub fn cloud_provider(&self) -> &dyn CloudProvider {
        (*self.cloud_provider).borrow()
    }

    pub fn dns_provider(&self) -> &dyn DnsProvider {
        (*self.dns_provider).borrow()
    }

    pub fn metrics_registry(&self) -> &dyn MetricsRegistry {
        self.metrics_registry.borrow()
    }

    pub fn is_valid(&self) -> Result<(), Box<EngineConfigError>> {
        if let Err(e) = self.cloud_provider.is_valid() {
            return Err(Box::new(EngineConfigError::CloudProviderNotValid(*e)));
        }

        if let Err(e) = self.dns_provider.is_valid() {
            return Err(Box::new(EngineConfigError::DnsProviderNotValid(
                e.to_engine_error(self.dns_provider.event_details()),
            )));
        }

        Ok(())
    }

    // The kubeconfig file may not exist yet on disk, so we create the client lazily
    pub fn mk_kube_client(&self) -> Result<QubeClient, Box<EngineError>> {
        if let Some(client) = self.kube_client.lock().unwrap().borrow().as_ref() {
            return Ok(client.clone());
        }

        let event_details = self
            .kubernetes()
            .get_event_details(Infrastructure(InfrastructureStep::RetrieveClusterResources));

        let kubeconfig_path = {
            let kubeconfig_path = self.kubernetes().kubeconfig_local_file_path();
            if kubeconfig_path.exists() {
                Some(kubeconfig_path)
            } else if self.is_infra_deployment {
                // Infra deployment must have a kubeconfig file, we cant upgrade infra within the cluster
                return Err(Box::new(EngineError::new_kubeconfig_file_do_not_match_the_current_cluster(
                    event_details.clone(),
                )));
            } else {
                None
            }
        };

        let kube_credentials: Vec<(String, String)> = self
            .cloud_provider
            .credentials_environment_variables()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        let client = QubeClient::new(event_details, kubeconfig_path, kube_credentials)?;

        *self.kube_client.lock().unwrap() = Some(client.clone());
        Ok(client)
    }
}

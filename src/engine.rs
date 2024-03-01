use std::borrow::Borrow;
use std::sync::Arc;
use thiserror::Error;

use crate::build_platform::BuildPlatform;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::CloudProvider;
use crate::container_registry::ContainerRegistry;
use crate::dns_provider::DnsProvider;
use crate::errors::EngineError;
use crate::io_models::context::Context;
use crate::metrics_registry::MetricsRegistry;

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
    cloud_provider: Arc<dyn CloudProvider>,
    dns_provider: Arc<dyn DnsProvider>,
    kubernetes: Box<dyn Kubernetes>,
    metrics_registry: Box<dyn MetricsRegistry>,
}

impl InfrastructureContext {
    pub fn new(
        context: Context,
        build_platform: Box<dyn BuildPlatform>,
        container_registry: Box<dyn ContainerRegistry>,
        cloud_provider: Arc<dyn CloudProvider>,
        dns_provider: Arc<dyn DnsProvider>,
        kubernetes: Box<dyn Kubernetes>,
        metrics_registry: Box<dyn MetricsRegistry>,
    ) -> InfrastructureContext {
        InfrastructureContext {
            context,
            build_platform,
            container_registry,
            cloud_provider,
            dns_provider,
            kubernetes,
            metrics_registry,
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
}

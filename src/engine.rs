use std::borrow::Borrow;
use std::sync::Arc;
use thiserror::Error;

use crate::build_platform::BuildPlatform;
use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::CloudProvider;
use crate::container_registry::ContainerRegistry;
use crate::dns_provider::errors::DnsProviderError;
use crate::dns_provider::DnsProvider;
use crate::errors::EngineError;
use crate::io_models::context::Context;

#[derive(Error, Debug, PartialEq, Eq)]
pub enum EngineConfigError {
    #[error("Build platform is not valid error: {0}")]
    BuildPlatformNotValid(EngineError),
    #[error("Cloud provider is not valid error: {0}")]
    CloudProviderNotValid(EngineError),
    #[error("DNS provider is not valid error: {0}")]
    DnsProviderNotValid(DnsProviderError),
    #[error("Kubernetes is not valid error: {0}")]
    KubernetesNotValid(EngineError),
}

pub struct EngineConfig {
    context: Context,
    build_platform: Box<dyn BuildPlatform>,
    container_registry: Box<dyn ContainerRegistry>,
    cloud_provider: Arc<Box<dyn CloudProvider>>,
    dns_provider: Arc<Box<dyn DnsProvider>>,
    kubernetes: Box<dyn Kubernetes>,
}

impl EngineConfig {
    pub fn new(
        context: Context,
        build_platform: Box<dyn BuildPlatform>,
        container_registry: Box<dyn ContainerRegistry>,
        cloud_provider: Arc<Box<dyn CloudProvider>>,
        dns_provider: Arc<Box<dyn DnsProvider>>,
        kubernetes: Box<dyn Kubernetes>,
    ) -> EngineConfig {
        EngineConfig {
            context,
            build_platform,
            container_registry,
            cloud_provider,
            dns_provider,
            kubernetes,
        }
    }

    pub fn kubernetes(&self) -> &dyn Kubernetes {
        self.kubernetes.as_ref()
    }

    pub fn context(&self) -> &Context {
        &self.context
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

    pub fn is_valid(&self) -> Result<(), EngineConfigError> {
        if let Err(e) = self.cloud_provider.is_valid() {
            return Err(EngineConfigError::CloudProviderNotValid(e));
        }

        if let Err(e) = self.dns_provider.is_valid() {
            return Err(EngineConfigError::DnsProviderNotValid(e));
        }

        Ok(())
    }
}

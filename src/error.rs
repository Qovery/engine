use crate::build_platform::error::BuildPlatformError;
use crate::cloud_provider::CloudProviderError;
use crate::container_registry::ContainerRegistryError;
use crate::dns_provider::DnsProviderError;

#[derive(Debug)]
pub enum ConfigurationError {
    BuildPlatform(BuildPlatformError),
    ContainerRegistry(ContainerRegistryError),
    CloudProvider(CloudProviderError),
    DnsProvider(DnsProviderError),
}

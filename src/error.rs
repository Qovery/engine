use crate::build_platform::error::BuildPlatformError;
use crate::cloud_provider::CloudProviderError;
use crate::container_registry::ContainerRegistryError;

#[derive(Debug)]
pub enum ConfigurationError {
    BuildPlatform(BuildPlatformError),
    ContainerRegistry(ContainerRegistryError),
    CloudProvider(CloudProviderError),
}

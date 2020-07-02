use crate::build_platform::error::BuildPlatformError;
use crate::cloud_provider::error::CloudProviderError;
use crate::container_registry::error::ContainerRegistryError;

#[derive(Debug)]
pub enum ConfigurationError {
    BuildPlatform(BuildPlatformError),
    ContainerRegistry(ContainerRegistryError),
    CloudProvider(CloudProviderError),
}

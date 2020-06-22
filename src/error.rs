use crate::build_platform::error::BuildPlatformError;
use crate::cloud_provider::error::CloudProviderError;
use crate::container_registry::error::ContainerRegistryError;
use crate::models::EnvironmentError;

#[derive(Debug)]
pub enum ConfigurationError {
    Environment(EnvironmentError),
    BuildPlatform(BuildPlatformError),
    ContainerRegistry(ContainerRegistryError),
    CloudProvider(CloudProviderError),
}

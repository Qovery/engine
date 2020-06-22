use crate::build_platform::BuildPlatform;
use crate::cloud_provider::{CloudProvider, Kubernetes};
use crate::config::ConfigError::{
    BuildPlatformError, CloudProviderError, ContainerRegistryError, EnvironmentError,
};
use crate::container_registry::ContainerRegistry;
use crate::models::Environment;
use crate::session::Session;

pub struct Config {
    pub environment: Environment,
    pub build_platform: Box<dyn BuildPlatform>,
    pub container_registry: Box<dyn ContainerRegistry>,
    pub cloud_provider: Box<dyn CloudProvider>,
}

impl<'a> Config {
    /// Read JSON and return a Config
    pub fn from_json(json: &str) -> Self {
        unimplemented!()
    }

    pub fn is_valid(&self) -> Result<(), ConfigError<'a>> {
        if !self.environment.is_valid() {
            return Err(EnvironmentError("there is an Environment error"));
        }

        if !self.build_platform.is_valid() {
            return Err(BuildPlatformError(
                "there is a Continuous integration error",
            ));
        }

        if !self.container_registry.is_valid() {
            return Err(ContainerRegistryError(
                "there is a Container Registry error",
            ));
        }

        if self.cloud_provider.is_valid().is_err() {
            return Err(CloudProviderError("there is a Cloud provider error"));
        }

        Ok(())
    }

    /// check and init the connection to all the services
    pub fn session(self) -> Result<Session, ConfigError<'a>> {
        match self.is_valid() {
            Ok(_) => Ok(Session { config: self }),
            Err(err) => Err(err),
        }
    }
}

type ErrorMessage<'a> = &'a str;

/// TODO change
pub enum ConfigError<'a> {
    EnvironmentError(ErrorMessage<'a>),
    BuildPlatformError(ErrorMessage<'a>),
    ContainerRegistryError(ErrorMessage<'a>),
    CloudProviderError(ErrorMessage<'a>),
}

#[cfg(test)]
mod tests {
    use crate::config::Config;

    #[test]
    fn read_from_json() {
        Config::from_json("");
    }
}

use crate::build_platform::BuildPlatform;
use crate::cloud_provider::{CloudProvider, Kubernetes};
use crate::container_registry::ContainerRegistry;
use crate::error::ConfigurationError;
use crate::models::Environment;
use crate::session::Session;

pub struct Config {
    pub build_platform: Box<dyn BuildPlatform>,
    pub container_registry: Box<dyn ContainerRegistry>,
    pub cloud_provider: Box<dyn CloudProvider>,
}

impl<'a> Config {
    /// Read JSON and return a Config
    pub fn from_json(json: &str) -> Self {
        unimplemented!()
    }

    pub fn is_valid(&self) -> Result<(), ConfigurationError> {
        match self.build_platform.is_valid() {
            Ok(_) => {}
            Err(err) => {
                return Err(ConfigurationError::BuildPlatform(err));
            }
        }

        match self.container_registry.is_valid() {
            Ok(_) => {}
            Err(err) => {
                return Err(ConfigurationError::ContainerRegistry(err));
            }
        }

        match self.cloud_provider.is_valid() {
            Ok(_) => {}
            Err(err) => {
                return Err(ConfigurationError::CloudProvider(err));
            }
        }

        Ok(())
    }

    /// check and init the connection to all the services
    pub fn session(self) -> Result<Session, ConfigurationError> {
        match self.is_valid() {
            Ok(_) => Ok(Session { config: self }),
            Err(err) => Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::Config;

    #[test]
    fn read_from_json() {
        Config::from_json("");
    }
}

use crate::build_platform::BuildPlatform;
use crate::cloud_provider::CloudProvider;
use crate::container_registry::ContainerRegistry;
use crate::error::ConfigurationError;
use crate::session::Session;
use std::borrow::Borrow;

pub struct Config {
    build_platform: Box<dyn BuildPlatform>,
    container_registry: Box<dyn ContainerRegistry>,
    cloud_provider: Box<dyn CloudProvider>,
}

impl Config {
    pub fn new(
        build_platform: Box<dyn BuildPlatform>,
        container_registry: Box<dyn ContainerRegistry>,
        cloud_provider: Box<dyn CloudProvider>,
    ) -> Config {
        Config {
            build_platform,
            container_registry,
            cloud_provider,
        }
    }
}

impl<'a> Config {
    /// Read JSON and return a Config
    pub fn from_json(json: &str) -> Self {
        unimplemented!()
    }

    pub fn build_platform(&self) -> &dyn BuildPlatform {
        self.build_platform.borrow()
    }

    pub fn container_registry(&self) -> &dyn ContainerRegistry {
        self.container_registry.borrow()
    }

    pub fn cloud_provider(&self) -> &dyn CloudProvider {
        self.cloud_provider.borrow()
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
    pub fn session(&'a self) -> Result<Session<'a>, ConfigurationError> {
        match self.is_valid() {
            Ok(_) => Ok(Session::<'a> { config: self }),
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

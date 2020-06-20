use crate::cloud_provider::{CloudProvider, Kubernetes};
use crate::config::ConfigError::{
    CloudProviderError, ContinuousIntegrationError, EnvironmentError, RegistryError,
};
use crate::continuous_integration::ContinuousIntegration;
use crate::models::Environment;
use crate::registry::Registry;
use crate::session::Session;

pub struct Config<'a, K>
where
    K: Kubernetes,
{
    pub environment: Environment,
    pub continuous_integration: Box<dyn ContinuousIntegration<'a, K>>,
    pub registry: Box<dyn Registry<'a>>,
    pub cloud_provider: Box<dyn CloudProvider<'a, K>>,
}

impl<'a, K> Config<'a, K>
where
    K: Kubernetes,
{
    /// Read JSON and return a Config
    pub fn from_json(json: &str) -> Self {
        unimplemented!()
    }

    pub fn is_valid(&self) -> Result<(), ConfigError<'a>> {
        if !self.environment.is_valid() {
            return Err(EnvironmentError("there is an Environment error"));
        }

        if !self.continuous_integration.is_valid() {
            return Err(ContinuousIntegrationError(
                "there is an ContinuousIntegration provider error",
            ));
        }

        if !self.registry.is_valid() {
            return Err(RegistryError("there is an Registry provider error"));
        }

        if !self.cloud_provider.is_valid() {
            return Err(CloudProviderError("there is an Cloud provider error"));
        }

        Ok(())
    }

    /// check and init the connection to all the services
    pub fn session(self) -> Result<Session<'a, K>, ConfigError<'a>> {
        match self.is_valid() {
            Ok(_) => Ok(Session::<'a, K> { config: self }),
            Err(err) => Err(err),
        }
    }
}

type ErrorMessage<'a> = &'a str;

pub enum ConfigError<'a> {
    EnvironmentError(ErrorMessage<'a>),
    ContinuousIntegrationError(ErrorMessage<'a>),
    RegistryError(ErrorMessage<'a>),
    CloudProviderError(ErrorMessage<'a>),
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::aws::kubernetes::EKS;
    use crate::config::Config;

    #[test]
    fn read_from_json() {
        Config::<EKS>::from_json("");
    }
}

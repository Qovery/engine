use crate::cloud_provider::{CloudProvider, Kubernetes};
use crate::config::ConfigError::{
    CloudProviderError, ContinuousIntegrationError, EnvironmentError, RegistryError,
};
use crate::continuous_integration::ContinuousIntegration;
use crate::models::Environment;
use crate::session::Session;

pub struct Config<'a> {
    pub environment: Environment,
    pub continuous_integration: Box<dyn ContinuousIntegration<'a>>,
    pub cloud_provider: Box<dyn CloudProvider<'a>>,
}

impl<'a> Config<'a> {
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

        if !self.cloud_provider.is_valid() {
            return Err(CloudProviderError("there is an Cloud provider error"));
        }

        Ok(())
    }

    /// check and init the connection to all the services
    pub fn session(self) -> Result<Session<'a>, ConfigError<'a>> {
        match self.is_valid() {
            Ok(_) => Ok(Session::<'a> { config: self }),
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

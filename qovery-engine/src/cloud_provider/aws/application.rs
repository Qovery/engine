use crate::build_platform::Image;
use crate::cloud_provider::service::{Create, Delete, Service, ServiceError, ServiceType};
use crate::cloud_provider::{CloudProvider, DeploymentTarget};

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct Application {
    pub id: String,
    pub name: String,
    pub image: Image,
}

impl Service for Application {
    fn service_type(&self) -> ServiceType {
        ServiceType::Application
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn version(&self) -> &str {
        self.image.commit_id.as_str()
    }

    fn is_valid(&self) -> Result<(), ServiceError> {
        Ok(())
    }
}

impl Create for Application {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        info!("EKS.application.on_create() called for {}", self.name());

        let environment = match target {
            DeploymentTarget::ManagedServices(_, environment) => environment,
            DeploymentTarget::SelfHosted(_, environment) => environment,
        };

        Ok(())
    }

    fn on_create_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        warn!(
            "EKS.application.on_create_error() called for {}",
            self.name()
        );

        // FIXME
        Ok(())
    }
}

impl Delete for Application {
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        info!("EKS.application.on_delete() called for {}", self.name());

        // FIXME
        Ok(())
    }

    fn on_delete_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        warn!(
            "EKS.application.on_delete_error() called for {}",
            self.name()
        );

        // FIXME
        Ok(())
    }
}

use crate::build_platform::Image;
use crate::cloud_provider::service::{
    Create, DatabaseOptions, DatabaseType, Delete, Service, ServiceError, ServiceType,
};
use crate::cloud_provider::{CloudProvider, DeploymentTarget};

pub struct PostgreSQL {
    pub id: String,
    pub name: String,
    pub version: String,
    pub options: DatabaseOptions,
}

impl Service for PostgreSQL {
    fn service_type(&self) -> ServiceType {
        ServiceType::Database(DatabaseType::PostgreSQL(&self.options))
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn version(&self) -> &str {
        self.version.as_str()
    }

    fn is_valid(&self) -> Result<(), ServiceError> {
        unimplemented!()
    }
}

impl Create for PostgreSQL {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        info!("AWS.PostgreSQL.on_create() called for {}", self.name());

        match target {
            DeploymentTarget::ManagedServices(x) => {}
            DeploymentTarget::SelfHosted(x) => {}
        }

        Ok(())
    }

    fn on_create_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        warn!(
            "AWS.PostgreSQL.on_create_error() called for {}",
            self.name()
        );

        Ok(())
    }
}

impl Delete for PostgreSQL {
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        info!("AWS.PostgreSQL.on_delete() called for {}", self.name());

        Ok(())
    }

    fn on_delete_error(&self, target: &DeploymentTarget) -> Result<(), ServiceError> {
        warn!(
            "AWS.PostgreSQL.on_create_error() called for {}",
            self.name()
        );

        Ok(())
    }
}

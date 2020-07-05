use crate::build_platform::Image;
use crate::cloud_provider::service::{
    Create, DatabaseOptions, DatabaseType, Delete, EnvironmentType, Service, ServiceError,
    ServiceType,
};
use crate::cloud_provider::CloudProvider;

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

    fn image(&self) -> &Image {
        unimplemented!()
    }

    fn environment_type(&self) -> EnvironmentType {
        unimplemented!()
    }
}

impl<'a> Create<'a> for PostgreSQL {
    fn on_create(&self, target: &'a dyn CloudProvider) {
        match self.environment_type() {
            EnvironmentType::Production => {}
            EnvironmentType::Development => {}
        }
    }

    fn on_create_error(&self, target: &'a dyn CloudProvider) {
        match self.environment_type() {
            EnvironmentType::Production => {}
            EnvironmentType::Development => {}
        }
    }
}

impl<'a> Delete<'a> for PostgreSQL {
    fn on_delete(&self, target: &'a dyn CloudProvider) {
        match self.environment_type() {
            EnvironmentType::Production => {}
            EnvironmentType::Development => {}
        }
    }

    fn on_delete_error(&self, target: &'a dyn CloudProvider) {
        match self.environment_type() {
            EnvironmentType::Production => {}
            EnvironmentType::Development => {}
        }
    }
}

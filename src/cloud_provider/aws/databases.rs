use crate::build_platform::Image;
use crate::cloud_provider::error::ServiceError;
use crate::cloud_provider::{
    CloudProvider, Create, DatabaseOptions, DatabaseType, Delete, EnvironmentType, Kubernetes,
    Service, ServiceType,
};

pub struct PostgreSQL<'a> {
    id: &'a str,
    name: &'a str,
    version: &'a str,
    options: DatabaseOptions<'a>,
}

impl<'a> Service for PostgreSQL<'a> {
    fn service_type(&self) -> ServiceType {
        ServiceType::Database(DatabaseType::PostgreSQL(&self.options))
    }

    fn id(&self) -> &str {
        self.id
    }

    fn name(&self) -> &str {
        self.name
    }

    fn version(&self) -> &str {
        self.version
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

impl<'a> Create for PostgreSQL<'a> {
    fn on_create(&self, target: Box<dyn CloudProvider>) {
        match self.environment_type() {
            EnvironmentType::Production => {}
            EnvironmentType::Development => {}
        }
    }

    fn on_create_error(&self, target: Box<dyn CloudProvider>) {
        match self.environment_type() {
            EnvironmentType::Production => {}
            EnvironmentType::Development => {}
        }
    }
}

impl<'a> Delete for PostgreSQL<'a> {
    fn on_delete(&self, target: Box<dyn CloudProvider>) {
        match self.environment_type() {
            EnvironmentType::Production => {}
            EnvironmentType::Development => {}
        }
    }

    fn on_delete_error(&self, target: Box<dyn CloudProvider>) {
        match self.environment_type() {
            EnvironmentType::Production => {}
            EnvironmentType::Development => {}
        }
    }
}

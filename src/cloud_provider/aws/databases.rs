use crate::cloud_provider::{
    CloudProvider, Create, DatabaseOptions, DatabaseType, EnvironmentType, Kubernetes, Service,
    ServiceType,
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

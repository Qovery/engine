use crate::build_platform::Image;
use crate::cloud_provider::service::{
    Create, Delete, EnvironmentType, Service, ServiceError, ServiceType,
};
use crate::cloud_provider::CloudProvider;

pub struct Router {
    pub id: String,
    pub name: String,
    pub custom_domains: Vec<CustomDomain>,
    pub routes: Vec<Route>,
}

impl<'a> Service for Router {
    fn service_type(&self) -> ServiceType {
        ServiceType::Router
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn version(&self) -> &str {
        unimplemented!()
    }

    fn is_valid(&self) -> Result<(), ServiceError> {
        unimplemented!()
    }

    fn environment_type(&self) -> EnvironmentType {
        unimplemented!()
    }
}

impl Create for Router {
    fn on_create(&self, target: &dyn CloudProvider) {
        // TODO custom domains? create an NGINX ingress
        // TODO render helm common config and apply
        unimplemented!()
    }

    fn on_create_error(&self, target: &dyn CloudProvider) {
        unimplemented!()
    }
}

impl Delete for Router {
    fn on_delete(&self, target: &dyn CloudProvider) {
        unimplemented!()
    }

    fn on_delete_error(&self, target: &dyn CloudProvider) {
        unimplemented!()
    }
}

pub struct CustomDomain {}

pub struct Route {}

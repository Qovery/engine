use crate::build_platform::Image;
use crate::cloud_provider::error::ServiceError;
use crate::cloud_provider::{CloudProvider, Create, Delete, EnvironmentType, Service, ServiceType};
use std::borrow::Borrow;

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct Application {
    pub id: String,
    pub name: String,
    pub image: Image,
}

impl<'a> Service for Application {
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
        unimplemented!()
    }

    fn image(&self) -> &Image {
        &self.image
    }

    fn environment_type(&self) -> EnvironmentType {
        unimplemented!()
    }
}

impl<'a> Create for Application {
    fn on_create(&self, target: Box<dyn CloudProvider>) {
        unimplemented!()
    }

    fn on_create_error(&self, target: Box<dyn CloudProvider>) {
        unimplemented!()
    }
}

impl<'a> Delete for Application {
    fn on_delete(&self, target: Box<dyn CloudProvider>) {
        unimplemented!()
    }

    fn on_delete_error(&self, target: Box<dyn CloudProvider>) {
        unimplemented!()
    }
}

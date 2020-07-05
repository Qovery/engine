use crate::build_platform::Image;
use crate::cloud_provider::service::{
    Create, Delete, EnvironmentType, Service, ServiceError, ServiceType,
};
use crate::cloud_provider::CloudProvider;

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

impl<'a> Create<'a> for Application {
    fn on_create(&self, target: &'a dyn CloudProvider) {
        unimplemented!()
    }

    fn on_create_error(&self, target: &'a dyn CloudProvider) {
        unimplemented!()
    }
}

impl<'a> Delete<'a> for Application {
    fn on_delete(&self, target: &'a dyn CloudProvider) {
        unimplemented!()
    }

    fn on_delete_error(&self, target: &'a dyn CloudProvider) {
        unimplemented!()
    }
}

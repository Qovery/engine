use crate::build_platform::Image;
use crate::container_registry::error::ContainerRegistryError;
use crate::container_registry::{ContainerRegistry, PushError, PushResult};

pub struct ECR {}

impl ContainerRegistry for ECR {
    fn is_valid(&self) -> Result<(), ContainerRegistryError> {
        unimplemented!()
    }

    fn on_create(&self) -> Result<(), ContainerRegistryError> {
        unimplemented!()
    }

    fn on_create_error(&self) -> Result<(), ContainerRegistryError> {
        unimplemented!()
    }

    fn on_delete(&self) -> Result<(), ContainerRegistryError> {
        unimplemented!()
    }

    fn on_delete_error(&self) -> Result<(), ContainerRegistryError> {
        unimplemented!()
    }

    fn push(&self, image: Image) -> Result<PushResult, PushError> {
        unimplemented!()
    }

    fn push_error(&self, image: Image) -> Result<PushResult, PushError> {
        unimplemented!()
    }
}

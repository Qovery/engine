use crate::build_platform::Image;
use crate::container_registry::error::ContainerRegistryError;
use crate::container_registry::{ContainerRegistry, PushError, PushResult};

pub struct ECR {
    access_key_id: String,
    secret_access_key: String,
}

impl ECR {
    pub fn new(access_key_id: &str, secret_access_key: &str) -> Self {
        ECR {
            access_key_id: access_key_id.to_string(),
            secret_access_key: secret_access_key.to_string(),
        }
    }
}

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

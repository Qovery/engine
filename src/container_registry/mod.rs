use crate::build_platform::Image;
use crate::container_registry::error::ContainerRegistryError;

pub mod docker_hub;
pub mod error;

pub trait ContainerRegistry {
    fn is_valid(&self) -> Result<(), ContainerRegistryError>;
    fn push(&self, image: &Image) -> Result<PushResult, PushError>;
}

pub struct PushResult {}

pub enum PushError {
    CredentialsError,
    ImageTagFailed,
    ImagePushFailed,
    ImageAlreadyExists,
}

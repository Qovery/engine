use crate::build_platform::Image;

pub mod docker_hub;

pub trait ContainerRegistry {
    fn is_valid(&self) -> bool;
    fn push(&self, image: &Image) -> Result<PushResult, PushError>;
}

pub struct PushResult {}

pub enum PushError {
    CredentialsError,
    ImageTagFailed,
    ImagePushFailed,
    ImageAlreadyExists,
}

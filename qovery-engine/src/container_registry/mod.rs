use rusoto_core::RusotoError;

use crate::build_platform::Image;
use serde::{Deserialize, Serialize};

pub mod docker_hub;
pub mod ecr;

pub trait ContainerRegistry {
    fn execution_id(&self) -> &str;
    fn kind(&self) -> Kind;
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn is_valid(&self) -> Result<(), ContainerRegistryError>;
    fn on_create(&self) -> Result<(), ContainerRegistryError>;
    fn on_create_error(&self) -> Result<(), ContainerRegistryError>;
    fn on_delete(&self) -> Result<(), ContainerRegistryError>;
    fn on_delete_error(&self) -> Result<(), ContainerRegistryError>;
    fn push(&self, image: Image) -> Result<PushResult, PushError>;
    fn push_error(&self, image: Image) -> Result<PushResult, PushError>;
}

pub struct PushResult {
    pub image: Image,
}

pub enum PushError {
    RepositoryInitFailure,
    CredentialsError,
    ImageTagFailed,
    ImagePushFailed,
    ImageAlreadyExists,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum Kind {
    DockerHub,
    ECR,
}

#[derive(Debug, Eq, PartialEq)]
pub enum ContainerRegistryError {
    Credentials,
    Unknown,
}

impl<E> From<RusotoError<E>> for ContainerRegistryError {
    fn from(error: RusotoError<E>) -> Self {
        match error {
            RusotoError::Credentials(_) => ContainerRegistryError::Credentials,
            RusotoError::Service(_) => ContainerRegistryError::Unknown,
            RusotoError::HttpDispatch(_) => ContainerRegistryError::Unknown,
            RusotoError::Validation(_) => ContainerRegistryError::Unknown,
            RusotoError::ParseError(_) => ContainerRegistryError::Unknown,
            RusotoError::Unknown(e) => {
                if e.status == 403 {
                    ContainerRegistryError::Credentials
                } else {
                    ContainerRegistryError::Unknown
                }
            }
            RusotoError::Blocking => ContainerRegistryError::Unknown,
        }
    }
}

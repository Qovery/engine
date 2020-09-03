use rusoto_core::RusotoError;

use crate::build_platform::Image;
use crate::models::{Context, ProgressListener};
use serde::{Deserialize, Serialize};
use std::rc::Rc;

pub mod docker_hub;
pub mod docr;
pub mod ecr;

pub trait ContainerRegistry {
    fn context(&self) -> &Context;
    fn kind(&self) -> Kind;
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn is_valid(&self) -> Result<(), ContainerRegistryError>;
    fn add_listener(&mut self, listener: Rc<Box<dyn ProgressListener>>);
    fn on_create(&self) -> Result<(), ContainerRegistryError>;
    fn on_create_error(&self) -> Result<(), ContainerRegistryError>;
    fn on_delete(&self) -> Result<(), ContainerRegistryError>;
    fn on_delete_error(&self) -> Result<(), ContainerRegistryError>;
    fn does_image_exists(&self, image: &Image) -> bool;
    fn push(&self, image: &Image, force_push: bool) -> Result<PushResult, PushError>;
    fn push_error(&self, image: &Image) -> Result<PushResult, PushError>;
}

pub struct PushResult {
    pub image: Image,
}

#[derive(Debug)]
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
    DOCR,
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

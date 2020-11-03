use std::error::Error;
use std::rc::Rc;

use rusoto_core::RusotoError;
use serde::{Deserialize, Serialize};

use crate::build_platform::Image;
use crate::models::{Context, Listener, ProgressListener};

pub mod docker_hub;
pub mod docr;
pub mod ecr;

pub trait ContainerRegistry {
    fn context(&self) -> &Context;
    fn kind(&self) -> Kind;
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn is_valid(&self) -> Result<(), ContainerRegistryError>;
    fn add_listener(&mut self, listener: Listener);
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
    IoError(std::io::Error),
    ImageTagFailed,
    ImagePushFailed,
    ImageAlreadyExists,
    Unknown(String),
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
    Permissions,
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

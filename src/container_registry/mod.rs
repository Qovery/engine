use serde::{Deserialize, Serialize};

use crate::build_platform::Image;
use crate::error::{EngineError, EngineErrorCause, EngineErrorScope};
use crate::models::{Context, Listen};

pub mod docker_hub;
pub mod docr;
pub mod ecr;
pub mod utilities;

pub trait ContainerRegistry: Listen {
    fn context(&self) -> &Context;
    fn kind(&self) -> Kind;
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn name_with_id(&self) -> String {
        format!("{} ({})", self.name(), self.id())
    }
    fn is_valid(&self) -> Result<(), EngineError>;
    fn on_create(&self) -> Result<(), EngineError>;
    fn on_create_error(&self) -> Result<(), EngineError>;
    fn on_delete(&self) -> Result<(), EngineError>;
    fn on_delete_error(&self) -> Result<(), EngineError>;
    fn does_image_exists(&self, image: &Image) -> bool;
    fn push(&self, image: &Image, force_push: bool) -> Result<PushResult, EngineError>;
    fn push_error(&self, image: &Image) -> Result<PushResult, EngineError>;
    fn engine_error_scope(&self) -> EngineErrorScope {
        EngineErrorScope::ContainerRegistry(self.id().to_string(), self.name().to_string())
    }
    fn engine_error(&self, cause: EngineErrorCause, message: String) -> EngineError {
        EngineError::new(
            cause,
            self.engine_error_scope(),
            self.context().execution_id(),
            Some(message),
        )
    }
}

pub struct PushResult {
    pub image: Image,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Kind {
    DockerHub,
    Ecr,
    Docr,
}

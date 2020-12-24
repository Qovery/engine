use serde::{Deserialize, Serialize};

use crate::error::{EngineError, EngineErrorCause, EngineErrorScope};
use crate::models::{Context, StringPath};
use std::fs::File;

pub mod s3;
pub mod spaces;

#[derive(Serialize, Deserialize, Clone)]
pub enum Kind {
    S3,
    Spaces,
}

pub trait ObjectStorage {
    fn context(&self) -> &Context;
    fn kind(&self) -> Kind;
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn name_with_id(&self) -> String {
        format!("{} ({})", self.name(), self.id())
    }
    fn is_valid(&self) -> Result<(), EngineError>;
    fn create_bucket<S>(&self, bucket_name: S) -> Result<(), EngineError>
    where
        S: Into<String>;
    fn delete_bucket<S>(&self, bucket_name: S) -> Result<(), EngineError>
    where
        S: Into<String>;
    fn get<T, S>(&self, bucket_name: T, object_key: S) -> Result<(StringPath, File), EngineError>
    where
        T: Into<String>,
        S: Into<String>;
    fn engine_error_scope(&self) -> EngineErrorScope {
        EngineErrorScope::ObjectStorage(self.id().to_string(), self.name().to_string())
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

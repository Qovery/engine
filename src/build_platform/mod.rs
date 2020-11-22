use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::error::{EngineError, EngineErrorCause, EngineErrorScope};
use crate::git::Credentials;
use crate::models::{Context, ProgressListener};

pub mod local_docker;

pub trait BuildPlatform {
    fn context(&self) -> &Context;
    fn kind(&self) -> Kind;
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn name_with_id(&self) -> String {
        format!("{} ({})", self.name(), self.id())
    }
    fn is_valid(&self) -> Result<(), EngineError>;
    fn add_listener(&mut self, listener: Rc<Box<dyn ProgressListener>>);
    fn build(&self, build: Build, force_build: bool) -> Result<BuildResult, EngineError>;
    fn build_error(&self, build: Build) -> Result<BuildResult, EngineError>;
    fn engine_error_scope(&self) -> EngineErrorScope {
        EngineErrorScope::BuildPlatform(self.id().to_string(), self.name().to_string())
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

pub struct Build {
    pub git_repository: GitRepository,
    pub image: Image,
    pub options: BuildOptions,
}

pub struct BuildOptions {
    pub environment_variables: Vec<EnvironmentVariable>,
}

pub struct EnvironmentVariable {
    pub key: String,
    pub value: String,
}

pub struct GitRepository {
    pub url: String,
    pub credentials: Option<Credentials>,
    pub commit_id: String,
    pub dockerfile_path: String,
}

#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct Image {
    pub application_id: String,
    pub name: String,
    pub tag: String,
    pub commit_id: String,
    pub registry_url: Option<String>,
}

impl Image {
    pub fn name_with_tag(&self) -> String {
        format!("{}:{}", self.name, self.tag)
    }
}

pub struct BuildResult {
    pub build: Build,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Kind {
    LocalDocker,
}

use serde::{Deserialize, Serialize};

use crate::error::{EngineError, EngineErrorCause, EngineErrorScope};
use crate::git::Credentials;
use crate::models::{Context, Listen};

pub mod local_docker;

pub trait BuildPlatform: Listen {
    fn context(&self) -> &Context;
    fn kind(&self) -> Kind;
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn name_with_id(&self) -> String {
        format!("{} ({})", self.name(), self.id())
    }
    fn is_valid(&self) -> Result<(), EngineError>;
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
    pub dockerfile_path: Option<String>,
    pub root_path: String,
}

#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct Image {
    pub application_id: String,
    pub name: String,
    pub tag: String,
    pub commit_id: String,
    // registry name where the image has been pushed: Optional
    pub registry_name: Option<String>,
    // registry docker json config: Optional
    pub registry_docker_json_config: Option<String>,
    // registry secret to pull image: Optional
    pub registry_secret: Option<String>,
    // complete registry URL where the image has been pushed
    pub registry_url: Option<String>,
}

impl Image {
    pub fn name_with_tag(&self) -> String {
        format!("{}:{}", self.name, self.tag)
    }
}

impl Default for Image {
    fn default() -> Self {
        Image {
            application_id: "".to_string(),
            name: "".to_string(),
            tag: "".to_string(),
            commit_id: "".to_string(),
            registry_name: None,
            registry_docker_json_config: None,
            registry_secret: None,
            registry_url: None,
        }
    }
}

pub struct BuildResult {
    pub build: Build,
}

impl BuildResult {
    pub fn new(build: Build) -> Self {
        BuildResult { build }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Kind {
    LocalDocker,
}

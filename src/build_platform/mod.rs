use crate::build_platform::error::BuildPlatformError;
use crate::cloud_provider::Kubernetes;
use crate::config::Config;
use crate::container_registry::{ContainerRegistry, PushError, PushResult};
use crate::git::Credentials;

pub mod error;
pub mod local_docker;

pub trait BuildPlatform {
    fn is_valid(&self) -> Result<(), BuildPlatformError>;
    fn build(&self, build: Build) -> Result<BuildResult, BuildError>;
}

pub struct Build {
    pub git_repository: GitRepository,
    pub image: Image,
}

pub struct GitRepository {
    pub url: String,
    pub credentials: Option<Credentials>,
    pub commit_id: Option<String>,
}

pub struct Image {
    pub name: String,
    pub tag: String,
    pub commit_id: String,
}

impl Image {
    pub fn name_with_tag(&self) -> String {
        format!("{}:{}", self.name, self.tag)
    }
}

pub struct BuildResult {
    pub build: Build,
}

pub enum BuildError {
    ImageAlreadyExists,
}

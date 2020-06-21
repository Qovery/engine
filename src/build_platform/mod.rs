use crate::build_platform::registry::{PushError, PushResult, Registry};
use crate::cloud_provider::Kubernetes;
use crate::config::Config;
use crate::git::Credentials;

pub mod local_docker;
pub mod registry;

pub trait BuildPlatform<'a> {
    fn is_valid(&self) -> bool;
    fn registry(self) -> Box<dyn Registry<'a>>;
    fn build(&self, build: Build) -> Result<BuildResult, BuildError>;
    fn push(&self, image: Image) -> Result<PushResult, PushError>;
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

pub struct BuildResult {
    pub build: Build,
}

pub enum BuildError {
    ImageAlreadyExists,
}

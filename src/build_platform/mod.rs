use crate::build_platform::registry::Registry;
use crate::cloud_provider::Kubernetes;
use crate::config::Config;

pub mod local_docker;
pub mod registry;

pub trait BuildPlatform<'a> {
    fn is_valid(&self) -> bool;
    fn registry(self) -> Box<dyn Registry<'a>>;
    fn build(&self, image: BuildImage<'a>) -> Result<BuildResult<'a>, BuildError>;
}

pub struct BuildImage<'a> {
    pub directory_path: &'a str,
    pub name: &'a str,
    pub tag: &'a str,
}

pub struct BuildResult<'a> {
    pub image: BuildImage<'a>,
}

pub enum BuildError {
    ImageAlreadyExists,
}

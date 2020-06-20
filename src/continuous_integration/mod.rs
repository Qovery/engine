use crate::cloud_provider::Kubernetes;
use crate::config::Config;

mod local;

pub trait ContinuousIntegration<'a, K>
where
    K: Kubernetes,
{
    fn is_valid(&self) -> bool;
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

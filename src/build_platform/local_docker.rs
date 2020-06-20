use crate::build_platform::registry::Registry;
use crate::build_platform::{BuildError, BuildImage, BuildPlatform, BuildResult};
use crate::cloud_provider::Kubernetes;
use crate::config::Config;

/// use Docker in local
pub struct LocalDocker<'a> {
    pub registry: Box<dyn Registry<'a>>,
}

impl<'a> BuildPlatform<'a> for LocalDocker<'a> {
    fn is_valid(&self) -> bool {
        true
    }

    fn registry(self) -> Box<dyn Registry<'a>> {
        self.registry
    }

    fn build(&self, image: BuildImage<'a>) -> Result<BuildResult<'a>, BuildError> {
        unimplemented!()
    }
}

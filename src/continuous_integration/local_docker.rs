use crate::cloud_provider::Kubernetes;
use crate::config::Config;
use crate::continuous_integration::registry::Registry;
use crate::continuous_integration::{BuildError, BuildImage, BuildResult, ContinuousIntegration};

/// use Docker in local
pub struct LocalDocker<'a> {
    pub registry: &'a dyn Registry<'a>,
}

impl<'a> ContinuousIntegration<'a> for LocalDocker<'a> {
    fn is_valid(&self) -> bool {
        true
    }

    fn registry(&self) -> &'a dyn Registry<'a> {
        self.registry
    }

    fn build(&self, image: BuildImage<'a>) -> Result<BuildResult<'a>, BuildError> {
        unimplemented!()
    }
}

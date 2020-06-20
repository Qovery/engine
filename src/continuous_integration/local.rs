use crate::cloud_provider::Kubernetes;
use crate::config::Config;
use crate::continuous_integration::{BuildError, BuildImage, BuildResult, ContinuousIntegration};

/// use Docker in local
pub struct Local {}

impl<'a, K> ContinuousIntegration<'a, K> for Local
where
    K: Kubernetes,
{
    fn is_valid(&self) -> bool {
        true
    }

    fn build(&self, image: BuildImage<'a>) -> Result<BuildResult<'a>, BuildError> {
        unimplemented!()
    }
}

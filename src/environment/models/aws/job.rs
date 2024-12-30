use crate::environment::models::job::Job;
use crate::environment::models::types::{ToTeraContext, AWS};
use crate::errors::EngineError;
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use tera::Context as TeraContext;

impl ToTeraContext for Job<AWS> {
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, Box<EngineError>> {
        Ok(TeraContext::from_serialize(self.default_tera_context(target)).unwrap_or_default())
    }
}
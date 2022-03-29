use crate::cloud_provider::DeploymentTarget;
use crate::errors::EngineError;
use crate::models::router::RouterImpl;
use crate::models::types::{ToTeraContext, AWS};
use tera::Context as TeraContext;

impl ToTeraContext for RouterImpl<AWS> {
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        self.default_tera_context(target)
    }
}

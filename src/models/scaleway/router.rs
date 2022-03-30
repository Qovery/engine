use crate::cloud_provider::DeploymentTarget;
use crate::errors::EngineError;
use crate::models::router::Router;
use crate::models::types::{ToTeraContext, SCW};
use tera::Context as TeraContext;

impl ToTeraContext for Router<SCW> {
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        self.default_tera_context(target)
    }
}

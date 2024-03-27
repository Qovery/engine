use crate::cloud_provider::DeploymentTarget;
use crate::errors::EngineError;
use crate::models::router::Router;
use crate::models::types::{OnPremise, ToTeraContext};
use tera::Context as TeraContext;

impl ToTeraContext for Router<OnPremise> {
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, Box<EngineError>> {
        self.default_tera_context(target)
    }
}

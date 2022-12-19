use crate::cloud_provider::DeploymentTarget;
use crate::errors::EngineError;
use crate::models::router::Router;
use crate::models::types::{AWSEc2, ToTeraContext};
use tera::Context as TeraContext;

impl ToTeraContext for Router<AWSEc2> {
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, Box<EngineError>> {
        self.default_tera_context(target)
    }
}

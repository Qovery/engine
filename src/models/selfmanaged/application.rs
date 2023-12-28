use crate::cloud_provider::DeploymentTarget;
use crate::errors::EngineError;
use crate::models::application::Application;
use crate::models::types::{SelfManaged, ToTeraContext};
use tera::Context as TeraContext;

impl ToTeraContext for Application<SelfManaged> {
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, Box<EngineError>> {
        let context = self.default_tera_context(target);
        Ok(TeraContext::from_serialize(context).unwrap())
    }
}

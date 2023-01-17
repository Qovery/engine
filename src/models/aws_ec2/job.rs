use crate::cloud_provider::DeploymentTarget;
use crate::errors::EngineError;
use crate::models::container::RegistryTeraContext;
use crate::models::job::Job;
use crate::models::types::{AWSEc2, ToTeraContext};
use tera::Context as TeraContext;

impl ToTeraContext for Job<AWSEc2> {
    fn to_tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, Box<EngineError>> {
        let mut context = self.default_tera_context(target);
        context.registry = Some(RegistryTeraContext {
            secret_name: "awsecr-cred".to_string(),
            docker_json_config: None,
        });

        Ok(TeraContext::from_serialize(context).unwrap_or_default())
    }
}

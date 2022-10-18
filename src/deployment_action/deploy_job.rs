use crate::cloud_provider::DeploymentTarget;
use crate::deployment_action::DeploymentAction;
use crate::errors::EngineError;
use crate::models::job::Job;
use crate::models::types::{CloudProvider, ToTeraContext};

impl<T: CloudProvider> DeploymentAction for Job<T>
where
    Job<T>: ToTeraContext,
{
    fn on_create(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        unimplemented!()
    }
    fn on_pause(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        unimplemented!()
    }
    fn on_delete(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        unimplemented!()
    }
}

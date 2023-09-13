use crate::cloud_provider::DeploymentTarget;
use crate::deployment_action::DeploymentAction;
use crate::errors::EngineError;
use crate::models::container::Container;
use crate::models::helm_chart::HelmChart;
use crate::models::types::{CloudProvider, ToTeraContext};

impl<T: CloudProvider> DeploymentAction for HelmChart<T>
where
    Container<T>: ToTeraContext,
{
    fn on_create(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn on_pause(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn on_delete(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn on_restart(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        Ok(())
    }
}

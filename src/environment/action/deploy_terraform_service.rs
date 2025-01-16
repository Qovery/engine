use crate::environment::action::DeploymentAction;
use crate::environment::models::terraform_service::TerraformService;
use crate::environment::models::types::CloudProvider;
use crate::errors::EngineError;
use crate::infrastructure::models::cloud_provider::DeploymentTarget;

impl<T: CloudProvider> DeploymentAction for TerraformService<T> {
    fn on_create(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        info!("Terraform service on_create().");
        Ok(())
    }

    fn on_pause(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        info!("Terraform service on_pause().");
        Ok(())
    }

    fn on_delete(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        info!("Terraform service on_delete().");
        Ok(())
    }

    fn on_restart(&self, _target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        info!("Terraform service on_restart().");
        Ok(())
    }
}

use crate::cloud_provider::service::Action;
use crate::cloud_provider::DeploymentTarget;
use crate::errors::EngineError;

pub mod pause_service;
#[cfg(test)]
mod test_utils;

pub trait DeploymentAction {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError>;
    fn on_create_check(&self) -> Result<(), EngineError> {
        Ok(())
    }
    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), EngineError>;
    fn on_pause_check(&self) -> Result<(), EngineError> {
        Ok(())
    }
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError>;
    fn on_delete_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn exec_action(&self, deployment_target: &DeploymentTarget, action: Action) -> Result<(), EngineError> {
        match action {
            Action::Create => self.on_create(deployment_target),
            Action::Delete => self.on_delete(deployment_target),
            Action::Pause => self.on_pause(deployment_target),
            Action::Nothing => Ok(()),
        }
    }

    fn exec_check_action(&self, action: Action) -> Result<(), EngineError> {
        match action {
            Action::Create => self.on_create_check(),
            Action::Delete => self.on_delete_check(),
            Action::Pause => self.on_pause_check(),
            Action::Nothing => Ok(()),
        }
    }
}

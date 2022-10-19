use crate::cloud_provider::service::Action;
use crate::cloud_provider::DeploymentTarget;
use crate::errors::EngineError;

mod check_dns;
mod deploy_application;
mod deploy_container;
mod deploy_database;
pub mod deploy_environment;
mod deploy_helm;
mod deploy_job;
pub mod deploy_namespace;
mod deploy_router;
mod deploy_terraform;
mod pause_service;
#[cfg(test)]
mod test_utils;

pub trait DeploymentAction {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError>;
    fn on_create_check(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        Ok(())
    }
    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), EngineError>;
    fn on_pause_check(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        Ok(())
    }
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError>;
    fn on_delete_check(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        Ok(())
    }

    fn exec_action(&self, deployment_target: &DeploymentTarget, action: Action) -> Result<(), EngineError> {
        match action {
            Action::Create => self.on_create(deployment_target),
            Action::Delete => self.on_delete(deployment_target),
            Action::Pause => self.on_pause(deployment_target),
        }
    }

    fn exec_check_action(&self, action: Action, target: &DeploymentTarget) -> Result<(), EngineError> {
        match action {
            Action::Create => self.on_create_check(target),
            Action::Delete => self.on_delete_check(target),
            Action::Pause => self.on_pause_check(target),
        }
    }
}

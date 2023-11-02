use crate::cloud_provider::service::Action;
use crate::cloud_provider::DeploymentTarget;
use crate::errors::EngineError;

mod check_dns;
mod deploy_application;
mod deploy_container;
mod deploy_database;
pub mod deploy_environment;
pub mod deploy_helm;
mod deploy_helm_chart;
mod deploy_job;
pub mod deploy_namespace;
mod deploy_router;
mod deploy_terraform;
mod pause_service;
mod restart_service;
#[cfg(test)]
mod test_utils;
mod utils;

pub trait DeploymentAction: Send + Sync {
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>>;
    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>>;
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>>;
    fn on_restart(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>>;
    fn exec_action(&self, deployment_target: &DeploymentTarget, action: Action) -> Result<(), Box<EngineError>> {
        match action {
            Action::Create => self.on_create(deployment_target),
            Action::Delete => self.on_delete(deployment_target),
            Action::Pause => self.on_pause(deployment_target),
            Action::Restart => self.on_restart(deployment_target),
        }
    }
}

#[derive(Clone, Debug)]
pub enum K8sResourceType {
    Deployment,
    StateFulSet,
    DaemonSet,
    Job,
    CronJob,
}

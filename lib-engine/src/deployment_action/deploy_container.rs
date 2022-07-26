use crate::cloud_provider::DeploymentTarget;
use crate::deployment_action::DeploymentAction;
use crate::errors::EngineError;
use crate::events::{EnvironmentStep, Stage};
use crate::models::container::Container;
use crate::models::types::CloudProvider;

impl<T: CloudProvider> DeploymentAction for Container<T> {
    fn on_create(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        println!("{:?}", event_details);
        todo!()
    }

    fn on_pause(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        todo!()
    }

    fn on_delete(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        todo!()
    }
}

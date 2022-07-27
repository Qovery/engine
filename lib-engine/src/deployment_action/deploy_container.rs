use crate::cloud_provider::service::{delete_pending_service, Action, Service};
use crate::cloud_provider::DeploymentTarget;
use crate::deployment_action::deploy_helm::HelmDeployment;
use crate::deployment_action::pause_service::PauseServiceAction;
use crate::deployment_action::DeploymentAction;
use crate::deployment_report::application::reporter::ApplicationDeploymentReporter;
use crate::deployment_report::execute_long_deployment;
use crate::errors::EngineError;
use crate::events::{EnvironmentStep, Stage};
use crate::models::container::Container;
use crate::models::types::{CloudProvider, ToTeraContext};
use std::path::PathBuf;
use std::time::Duration;

impl<T: CloudProvider> DeploymentAction for Container<T>
where
    Container<T>: ToTeraContext,
{
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));

        execute_long_deployment(
            ApplicationDeploymentReporter::new_for_container(self, target, Action::Create),
            || {
                let helm = HelmDeployment::new(
                    self.helm_release_name(),
                    self.to_tera_context(target)?,
                    PathBuf::from(self.helm_chart_dir()),
                    PathBuf::from(self.workspace_directory()),
                    event_details.clone(),
                    Some(self.selector()),
                );

                helm.on_create(target)?;

                delete_pending_service(
                    target.kubernetes.get_kubeconfig_file_path()?.as_str(),
                    target.environment.namespace(),
                    self.selector().as_str(),
                    target.kubernetes.cloud_provider().credentials_environment_variables(),
                    event_details.clone(),
                )?;

                Ok(())
            },
        )
    }

    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        execute_long_deployment(
            ApplicationDeploymentReporter::new_for_container(self, target, Action::Pause),
            || {
                let pause_service = PauseServiceAction::new(
                    self.selector(),
                    self.is_stateful(),
                    Duration::from_secs(5 * 60),
                    self.get_event_details(Stage::Environment(EnvironmentStep::Pause)),
                );
                pause_service.on_pause(target)
            },
        )
    }

    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        execute_long_deployment(
            ApplicationDeploymentReporter::new_for_container(self, target, Action::Delete),
            || {
                let helm = HelmDeployment::new(
                    self.helm_release_name(),
                    self.to_tera_context(target)?,
                    PathBuf::from(self.helm_chart_dir()),
                    PathBuf::from(self.workspace_directory()),
                    self.get_event_details(Stage::Environment(EnvironmentStep::Delete)),
                    Some(self.selector()),
                );

                helm.on_delete(target)
            },
        )
    }
}

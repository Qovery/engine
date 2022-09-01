use crate::cloud_provider::service::{delete_pending_service, Action, Service};
use crate::cloud_provider::utilities::print_action;
use crate::cloud_provider::DeploymentTarget;
use crate::deployment_action::deploy_helm::HelmDeployment;
use crate::deployment_action::pause_service::PauseServiceAction;
use crate::deployment_action::DeploymentAction;
use crate::deployment_report::application::reporter::ApplicationDeploymentReporter;
use crate::deployment_report::execute_long_deployment;
use crate::deployment_report::logger::get_loggers;
use crate::errors::{CommandError, EngineError};
use crate::events::{EnvironmentStep, Stage};
use crate::kubers_utils::kube_delete_all_from_selector;
use crate::models::application::{Application, ApplicationService};
use crate::models::types::{CloudProvider, ToTeraContext};
use crate::runtime::block_on;
use function_name::named;
use k8s_openapi::api::core::v1::PersistentVolumeClaim;
use std::path::PathBuf;
use std::time::Duration;
use tera::Context;

impl<T: CloudProvider> DeploymentAction for Application<T>
where
    Application<T>: ToTeraContext,
{
    #[named]
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        print_action(
            T::short_name(),
            "application",
            function_name!(),
            self.name(),
            event_details.clone(),
            self.logger(),
        );

        execute_long_deployment(ApplicationDeploymentReporter::new(self, target, Action::Create), || {
            // If the service have been paused, we must ensure we un-pause it first as hpa will not kick in
            let _ = PauseServiceAction::new(
                self.selector(),
                self.is_stateful(),
                Duration::from_secs(5 * 60),
                event_details.clone(),
            )
            .unpause_if_needed(target);

            let helm = HelmDeployment::new(
                self.helm_release_name(),
                self.to_tera_context(target)?,
                PathBuf::from(self.helm_chart_dir()),
                PathBuf::from(self.workspace_directory()),
                event_details.clone(),
                Some(self.selector()),
                Some(self.startup_timeout()),
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
        })
    }

    #[named]
    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Pause));
        print_action(
            T::short_name(),
            "application",
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );

        execute_long_deployment(ApplicationDeploymentReporter::new(self, target, Action::Pause), || {
            let pause_service = PauseServiceAction::new(
                self.selector(),
                self.is_stateful(),
                Duration::from_secs(5 * 60),
                self.get_event_details(Stage::Environment(EnvironmentStep::Pause)),
            );
            pause_service.on_pause(target)
        })
    }

    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Delete));
        execute_long_deployment(ApplicationDeploymentReporter::new(self, target, Action::Delete), || {
            let helm = HelmDeployment::new(
                self.helm_release_name(),
                Context::default(),
                PathBuf::from(self.helm_chart_dir()),
                PathBuf::from(self.workspace_directory()),
                event_details.clone(),
                Some(self.selector()),
                None,
            );

            helm.on_delete(target)?;

            let logger = get_loggers(self, Action::Delete);
            // Delete pvc of statefulset if needed
            // FIXME: Remove this after kubernetes 1.23 is deployed, at it should be done by kubernetes
            if self.is_stateful() {
                (logger.send_progress)("🪓 Terminating network volume of the application".to_string());
                if let Err(err) = block_on(kube_delete_all_from_selector::<PersistentVolumeClaim>(
                    &target.kube,
                    &self.selector(),
                    target.environment.namespace(),
                )) {
                    return Err(EngineError::new_k8s_cannot_delete_pvcs(
                        event_details.clone(),
                        self.selector(),
                        CommandError::new_from_safe_message(err.to_string()),
                    ));
                }
            }

            // Delete container repository created for this application
            (logger.send_progress)("🪓 Terminating container registry of the application".to_string());
            if let Err(err) = target
                .container_registry
                .delete_repository(self.build().image.repository_name())
            {
                let engine_err = EngineError::new_container_registry_error(event_details.clone(), err.clone());

                let user_error = EngineError::new_engine_error(
                    engine_err.clone(),
                    format!("❌ Failed to delete container registry of the application: {}", err),
                    None,
                );
                (logger.send_error)(user_error);

                return Err(engine_err);
            }

            Ok(())
        })
    }
}

use crate::cloud_provider::helm::{ChartInfo, HelmAction, HelmChartNamespaces};
use crate::cloud_provider::service::{delete_pending_service, Action, Service};
use crate::cloud_provider::DeploymentTarget;
use crate::deployment_action::deploy_helm::HelmDeployment;
use crate::deployment_action::pause_service::PauseServiceAction;
use crate::deployment_action::DeploymentAction;
use crate::deployment_report::application::reporter::ApplicationDeploymentReporter;
use crate::deployment_report::execute_long_deployment;
use crate::errors::{CommandError, EngineError};
use crate::events::{EnvironmentStep, Stage};
use crate::kubers_utils::kube_delete_all_from_selector;
use crate::models::application::{Application, ApplicationService};
use crate::models::types::{CloudProvider, ToTeraContext};
use crate::runtime::block_on;
use k8s_openapi::api::core::v1::PersistentVolumeClaim;

use crate::deployment_report::logger::EnvProgressLogger;
use std::path::PathBuf;
use std::time::Duration;
use tera::Context;

impl<T: CloudProvider> DeploymentAction for Application<T>
where
    Application<T>: ToTeraContext,
{
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let long_task = |_logger: &EnvProgressLogger| -> Result<(), EngineError> {
            let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
            // If the service have been paused, we must ensure we un-pause it first as hpa will not kick in
            let _ = PauseServiceAction::new(
                self.selector(),
                self.is_stateful(),
                Duration::from_secs(5 * 60),
                event_details.clone(),
            )
            .unpause_if_needed(target);

            let chart = ChartInfo {
                name: self.helm_release_name(),
                path: self.workspace_directory().to_string(),
                namespace: HelmChartNamespaces::Custom,
                custom_namespace: Some(target.environment.namespace().to_string()),
                timeout_in_seconds: self.startup_timeout().as_secs() as i64,
                k8s_selector: Some(self.selector()),
                ..Default::default()
            };

            let helm = HelmDeployment::new(
                event_details.clone(),
                self.to_tera_context(target)?,
                PathBuf::from(self.helm_chart_dir()),
                None,
                chart,
            );

            helm.on_create(target)?;

            delete_pending_service(
                target.kubernetes.get_kubeconfig_file_path()?.as_str(),
                target.environment.namespace(),
                self.selector().as_str(),
                target.kubernetes.cloud_provider().credentials_environment_variables(),
                event_details,
            )?;

            Ok(())
        };

        execute_long_deployment(ApplicationDeploymentReporter::new(self, target, Action::Create), long_task)
    }

    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        execute_long_deployment(
            ApplicationDeploymentReporter::new(self, target, Action::Pause),
            |_logger: &EnvProgressLogger| -> Result<(), EngineError> {
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
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Delete));
        execute_long_deployment(
            ApplicationDeploymentReporter::new(self, target, Action::Delete),
            |logger: &EnvProgressLogger| {
                let chart = ChartInfo {
                    name: self.helm_release_name(),
                    namespace: HelmChartNamespaces::Custom,
                    custom_namespace: Some(target.environment.namespace().to_string()),
                    action: HelmAction::Destroy,
                    k8s_selector: Some(self.selector()),
                    ..Default::default()
                };
                let helm = HelmDeployment::new(
                    event_details.clone(),
                    Context::default(),
                    PathBuf::from(self.helm_chart_dir().as_str()),
                    None,
                    chart,
                );

                helm.on_delete(target)?;

                // Delete pvc of statefulset if needed
                // FIXME: Remove this after kubernetes 1.23 is deployed, at it should be done by kubernetes
                if self.is_stateful() {
                    logger.info("🪓 Terminating network volume of the application".to_string());
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
                logger.info("🪓 Terminating container registry of the application".to_string());
                if let Err(err) = target
                    .container_registry
                    .delete_repository(self.build().image.repository_name())
                {
                    let user_msg = format!("❌ Failed to delete container registry of the application: {}", err);
                    let user_error = EngineError::new_engine_error(
                        EngineError::new_container_registry_error(event_details.clone(), err),
                        user_msg,
                        None,
                    );
                    return Err(user_error);
                }

                Ok(())
            },
        )
    }
}

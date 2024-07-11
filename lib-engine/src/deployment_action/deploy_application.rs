use crate::cloud_provider::helm::{ChartInfo, HelmAction, HelmChartNamespaces};
use crate::cloud_provider::service::{Action, Service};
use crate::cloud_provider::{DeploymentTarget, Kind};
use crate::deployment_action::deploy_helm::HelmDeployment;
use crate::deployment_action::pause_service::PauseServiceAction;
use crate::deployment_action::DeploymentAction;
use crate::deployment_report::application::reporter::ApplicationDeploymentReporter;
use crate::deployment_report::execute_long_deployment;
use crate::errors::{CommandError, EngineError};
use crate::events::{EngineEvent, EnvironmentStep, EventMessage, Stage};
use crate::kubers_utils::{kube_delete_all_from_selector, KubeDeleteMode};
use crate::models::application::{get_application_with_invalid_storage_size, Application, ApplicationService};
use crate::models::types::{CloudProvider, ToTeraContext};
use crate::runtime::block_on;
use k8s_openapi::api::core::v1::PersistentVolumeClaim;

use crate::cloud_provider::utilities::update_pvcs;
use crate::deployment_action::restart_service::RestartServiceAction;
use crate::deployment_report::logger::EnvProgressLogger;
use std::path::PathBuf;
use std::time::Duration;
use tera::Context;

use super::utils::delete_nlb_or_alb_service;

impl<T: CloudProvider> DeploymentAction for Application<T>
where
    Application<T>: ToTeraContext,
{
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let long_task = |_logger: &EnvProgressLogger| -> Result<(), Box<EngineError>> {
            let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
            // If the service have been paused, we must ensure we un-pause it first as hpa will not kick in
            let _ = PauseServiceAction::new(
                self.kube_label_selector(),
                self.is_stateful(),
                Duration::from_secs(5 * 60),
                event_details.clone(),
            )
            .unpause_if_needed(target);

            match get_application_with_invalid_storage_size(
                self,
                &target.kube,
                target.environment.namespace(),
                &event_details,
            ) {
                Ok(invalid_statefulset_storage) => {
                    if let Some(invalid_statefulset_storage) = invalid_statefulset_storage {
                        update_pvcs(
                            self.as_service(),
                            &invalid_statefulset_storage,
                            target.environment.namespace(),
                            &event_details,
                            &target.kube,
                        )?;
                    }
                }
                Err(e) => target.kubernetes.logger().log(EngineEvent::Warning(
                    event_details.clone(),
                    EventMessage::new_from_safe(e.to_string()),
                )),
            };

            let chart = ChartInfo {
                name: self.helm_release_name(),
                path: self.workspace_directory().to_string(),
                namespace: HelmChartNamespaces::Custom,
                custom_namespace: Some(target.environment.namespace().to_string()),
                timeout_in_seconds: self.startup_timeout().as_secs() as i64,
                k8s_selector: Some(self.kube_label_selector()),
                ..Default::default()
            };

            let helm = HelmDeployment::new(
                event_details.clone(),
                self.to_tera_context(target)?,
                PathBuf::from(self.helm_chart_dir()),
                None,
                chart,
            );

            if target.cloud_provider.kind() == Kind::Aws {
                delete_nlb_or_alb_service(
                    target.qube_client(event_details.clone())?,
                    target.environment.namespace(),
                    format!("qovery.com/service-id={}", self.long_id()).as_str(),
                    target.kubernetes.advanced_settings().aws_eks_enable_alb_controller,
                    event_details,
                )?;
            }

            helm.on_create(target)?;

            Ok(())
        };

        execute_long_deployment(ApplicationDeploymentReporter::new(self, target, Action::Create), long_task)
    }

    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        execute_long_deployment(
            ApplicationDeploymentReporter::new(self, target, Action::Pause),
            |_logger: &EnvProgressLogger| -> Result<(), Box<EngineError>> {
                let pause_service = PauseServiceAction::new(
                    self.kube_label_selector(),
                    self.is_stateful(),
                    Duration::from_secs(5 * 60),
                    self.get_event_details(Stage::Environment(EnvironmentStep::Pause)),
                );
                pause_service.on_pause(target)
            },
        )
    }

    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Delete));
        execute_long_deployment(
            ApplicationDeploymentReporter::new(self, target, Action::Delete),
            |logger: &EnvProgressLogger| {
                let chart = ChartInfo {
                    name: self.helm_release_name(),
                    namespace: HelmChartNamespaces::Custom,
                    custom_namespace: Some(target.environment.namespace().to_string()),
                    action: HelmAction::Destroy,
                    k8s_selector: Some(self.kube_label_selector()),
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

                // Delete PVC of statefulset if needed
                // FIXME(ENG-1606): Remove this after kubernetes 1.23 is deployed, at it should be done by kubernetes
                if self.is_stateful() {
                    logger.info("🪓 Terminating network volume of the application".to_string());
                    // Trying to delete PVCs using new labels
                    if let Err(err) = block_on(kube_delete_all_from_selector::<PersistentVolumeClaim>(
                        &target.kube,
                        &self.kube_label_selector(),
                        target.environment.namespace(),
                        KubeDeleteMode::Normal,
                    )) {
                        return Err(Box::new(EngineError::new_k8s_cannot_delete_pvcs(
                            event_details.clone(),
                            self.kube_label_selector(),
                            CommandError::new_from_safe_message(err.to_string()),
                        )));
                    }
                    // Trying to delete PVCs using old labels
                    // TODO(benjaminch): should be removed once PVCs are migrated to new labels
                    if let Err(err) = block_on(kube_delete_all_from_selector::<PersistentVolumeClaim>(
                        &target.kube,
                        &self.kube_legacy_label_selector(),
                        target.environment.namespace(),
                        KubeDeleteMode::Normal,
                    )) {
                        return Err(Box::new(EngineError::new_k8s_cannot_delete_pvcs(
                            event_details.clone(),
                            self.kube_legacy_label_selector(),
                            CommandError::new_from_safe_message(err.to_string()),
                        )));
                    }
                }

                // Delete container repository created for this application
                logger.info("🪓 Terminating container registry of the application".to_string());
                if let Err(err) = target
                    .container_registry
                    .delete_repository(self.build().image.repository_name())
                {
                    let safe_user_msg = "❌ Failed to delete container registry of the application".to_string();
                    let user_error = EngineError::new_engine_error(
                        EngineError::new_container_registry_error(event_details.clone(), err),
                        safe_user_msg,
                        None,
                    );
                    return Err(Box::new(user_error));
                }

                Ok(())
            },
        )
    }

    fn on_restart(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        execute_long_deployment(
            ApplicationDeploymentReporter::new(self, target, Action::Restart),
            |_logger: &EnvProgressLogger| -> Result<(), Box<EngineError>> {
                let restart_service = RestartServiceAction::new(
                    self.kube_label_selector(),
                    self.is_stateful(),
                    self.get_event_details(Stage::Environment(EnvironmentStep::Restart)),
                );
                restart_service.on_restart(target)
            },
        )
    }
}

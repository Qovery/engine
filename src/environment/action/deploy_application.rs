use crate::environment::action::DeploymentAction;
use crate::environment::action::deploy_helm::HelmDeployment;
use crate::environment::action::pause_service::PauseServiceAction;
use crate::environment::models::application::{
    Application, ApplicationService, get_application_with_invalid_storage_size,
};
use crate::environment::models::types::{CloudProvider, ToTeraContext};
use crate::environment::report::application::reporter::ApplicationDeploymentReporter;
use crate::environment::report::execute_long_deployment;
use crate::errors::{CommandError, EngineError};
use crate::events::{EnvironmentStep, Stage};
use crate::helm::{ChartInfo, HelmAction, HelmChartNamespaces};
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::infrastructure::models::cloud_provider::service::{Action, Service};
use crate::kubers_utils::{KubeDeleteMode, kube_delete_all_from_selector};
use crate::runtime::block_on;
use k8s_openapi::api::core::v1::PersistentVolumeClaim;

use super::utils::{delete_nlb_or_alb_service, update_pvcs};
use crate::environment::action::restart_service::RestartServiceAction;
use crate::environment::report::logger::EnvProgressLogger;
use crate::infrastructure::models::kubernetes::Kind;
use std::path::PathBuf;
use std::time::Duration;
use tera::Context;

impl<T: CloudProvider> DeploymentAction for Application<T>
where
    Application<T>: ToTeraContext,
{
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let long_task = |logger: &EnvProgressLogger| -> Result<(), Box<EngineError>> {
            let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
            // If the service have been paused, we must ensure we un-pause it first as hpa will not kick in
            let _ = PauseServiceAction::new(
                self.kube_label_selector(),
                self.is_stateful(),
                Duration::from_secs(5 * 60),
                event_details.clone(),
                true,
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
                Err(e) => logger.warning(e.to_string()),
            };

            let chart = ChartInfo {
                name: self.helm_release_name(),
                path: self.workspace_directory().to_string(),
                namespace: HelmChartNamespaces::Custom(target.environment.namespace().to_string()),
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

            if target.kubernetes.kind() == Kind::Eks {
                delete_nlb_or_alb_service(
                    target.kube.clone(),
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
                    true,
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
                    namespace: HelmChartNamespaces::Custom(target.environment.namespace().to_string()),
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

                // Delete shared container repository if needed (depending on flag computed on core)
                if self.should_delete_shared_registry() {
                    logger.info("🪓 Terminating shared container registry of the application".to_string());
                    if let Err(err) = target
                        .container_registry
                        .delete_repository(self.build().image.shared_repository_name())
                    {
                        let safe_user_msg =
                            "❌ Failed to delete shared container registry of the application".to_string();
                        let user_error = EngineError::new_engine_error(
                            EngineError::new_container_registry_error(event_details.clone(), err),
                            safe_user_msg,
                            None,
                        );
                        return Err(Box::new(user_error));
                    }
                }

                // Delete container repository created for this application
                logger.info("🪓 Terminating container registry of the application".to_string());
                if let Err(err) = target
                    .container_registry
                    .delete_repository(self.build().image.legacy_repository_name())
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

use crate::environment::action::DeploymentAction;
use crate::environment::action::deploy_helm::HelmDeployment;
use crate::environment::action::pause_service::PauseServiceAction;
use crate::environment::models::container::{Container, ContainerService, get_container_with_invalid_storage_size};
use crate::environment::models::types::{CloudProvider, ToTeraContext};
use crate::environment::report::application::reporter::ApplicationDeploymentReporter;
use crate::environment::report::{DeploymentTaskImpl, execute_long_deployment};
use crate::errors::{CommandError, EngineError};
use crate::events::{EnvironmentStep, Stage};
use crate::helm::{ChartInfo, HelmAction, HelmChartNamespaces};
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::infrastructure::models::cloud_provider::service::{Action, Service};
use crate::kubers_utils::{KubeDeleteMode, kube_delete_all_from_selector};
use crate::runtime::block_on;
use k8s_openapi::api::core::v1::PersistentVolumeClaim;

use crate::environment::action::restart_service::RestartServiceAction;
use crate::environment::action::utils::{
    KubeObjectKind, delete_cached_image, delete_nlb_or_alb_service, get_last_deployed_image, mirror_image_if_necessary,
    update_pvcs,
};
use crate::environment::report::logger::{EnvProgressLogger, EnvSuccessLogger};
use crate::infrastructure::models::kubernetes;
use std::path::PathBuf;
use std::time::Duration;

impl<T: CloudProvider> DeploymentAction for Container<T>
where
    Container<T>: ToTeraContext,
{
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        let metrics_registry = target.metrics_registry.clone();
        struct TaskContext {
            last_deployed_image: Option<String>,
        }

        // We first mirror the image if needed
        let pre_task = |logger: &EnvProgressLogger| -> Result<TaskContext, Box<EngineError>> {
            mirror_image_if_necessary(
                self.long_id(),
                &self.source,
                target,
                logger,
                event_details.clone(),
                metrics_registry.clone(),
            )?;

            let last_image = block_on(get_last_deployed_image(
                target.kube.client(),
                &self.kube_label_selector(),
                if self.is_stateful() {
                    KubeObjectKind::Statefulset
                } else {
                    KubeObjectKind::Deployment
                },
                target.environment.namespace(),
            ));

            Ok(TaskContext {
                last_deployed_image: last_image,
            })
        };

        let long_task = |logger: &EnvProgressLogger, state: TaskContext| -> Result<TaskContext, Box<EngineError>> {
            // If the service have been paused, we must ensure we un-pause it first as hpa will not kick in
            let _ = PauseServiceAction::new(
                self.kube_label_selector(),
                self.is_stateful(),
                Duration::from_secs(5 * 60),
                event_details.clone(),
                true,
            )
            .unpause_if_needed(target);

            match get_container_with_invalid_storage_size(
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

            if target.kubernetes.kind() == kubernetes::Kind::Eks {
                delete_nlb_or_alb_service(
                    target.kube.clone(),
                    target.environment.namespace(),
                    format!("qovery.com/service-id={}", self.long_id()).as_str(),
                    target.kubernetes.advanced_settings().aws_eks_enable_alb_controller,
                    event_details.clone(),
                )?;
            }

            helm.on_create(target)?;

            Ok(state)
        };

        let post_task = |logger: &EnvSuccessLogger, state: TaskContext| {
            // Delete previous image from cache to cleanup resources
            let _ = delete_cached_image(
                self.long_id(),
                self.source.tag_for_mirror(self.long_id()),
                state.last_deployed_image,
                false,
                target,
                &|msg| logger.send_success(msg),
            )
            .map_err(|err| {
                error!("Error while deleting cached image: {}", err);
                Box::new(EngineError::new_container_registry_error(event_details.clone(), err))
            });
        };

        // At last we deploy our container
        execute_long_deployment(
            ApplicationDeploymentReporter::new_for_container(self, target, Action::Create),
            DeploymentTaskImpl {
                pre_run: &pre_task,
                run: &long_task,
                post_run_success: &post_task,
            },
        )
    }

    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        execute_long_deployment(
            ApplicationDeploymentReporter::new_for_container(self, target, Action::Pause),
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
        struct TaskContext {
            last_deployed_image: Option<String>,
        }

        // We first mirror the image if needed
        let pre_task = |_logger: &EnvProgressLogger| -> Result<TaskContext, Box<EngineError>> {
            let last_image = block_on(get_last_deployed_image(
                target.kube.client(),
                &self.kube_label_selector(),
                if self.is_stateful() {
                    KubeObjectKind::Statefulset
                } else {
                    KubeObjectKind::Deployment
                },
                target.environment.namespace(),
            ));

            Ok(TaskContext {
                last_deployed_image: last_image,
            })
        };

        // Execute the deployment
        let long_task = |logger: &EnvProgressLogger, state: TaskContext| -> Result<TaskContext, Box<EngineError>> {
            let chart = ChartInfo {
                name: self.helm_release_name(),
                namespace: HelmChartNamespaces::Custom(target.environment.namespace().to_string()),
                action: HelmAction::Destroy,
                ..Default::default()
            };
            let helm = HelmDeployment::new(
                event_details.clone(),
                self.to_tera_context(target)?,
                PathBuf::from(self.helm_chart_dir().as_str()),
                None,
                chart,
            );

            helm.on_delete(target)?;

            // Delete pvc of statefulset if needed
            // FIXME(ENG-1606): Remove this after kubernetes 1.23 is deployed, at it should be done by kubernetes
            if self.is_stateful() {
                logger.info("🪓 Terminating network volume of the container".to_string());
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

            Ok(state)
        };

        // Cleanup the image from the cache
        let post_task = |logger: &EnvSuccessLogger, state: TaskContext| {
            // Delete previous image from cache to cleanup resources
            let last_deployed_image = if state.last_deployed_image.is_none() {
                Some(self.source.tag_for_mirror(self.long_id()))
            } else {
                state.last_deployed_image
            };

            let _ = delete_cached_image(
                self.long_id(),
                self.source.tag_for_mirror(self.long_id()),
                last_deployed_image,
                true,
                target,
                &|msg| logger.send_success(msg),
            )
            .map_err(|err| {
                error!("Error while deleting cached image: {}", err);
                Box::new(EngineError::new_container_registry_error(event_details.clone(), err))
            });
        };

        // Trigger deployment
        execute_long_deployment(
            ApplicationDeploymentReporter::new_for_container(self, target, Action::Delete),
            DeploymentTaskImpl {
                pre_run: &pre_task,
                run: &long_task,
                post_run_success: &post_task,
            },
        )
    }

    fn on_restart(&self, target: &DeploymentTarget) -> Result<(), Box<EngineError>> {
        execute_long_deployment(
            ApplicationDeploymentReporter::new_for_container(self, target, Action::Restart),
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

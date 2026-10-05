use crate::environment::action::DeploymentAction;
use crate::environment::action::deploy_helm::{HelmChartValues, HelmDeployment};
use crate::environment::action::pause_service::PauseServiceAction;
use crate::environment::models::container::{Container, ContainerService};
use crate::environment::models::types::{CloudProvider, ToTeraContext};
use crate::environment::report::application::reporter::ApplicationDeploymentReporter;
use crate::environment::report::{DeploymentTaskRef, execute_long_deployment};
use crate::errors::EngineError;
use crate::events::{EnvironmentStep, Stage};
use crate::helm::{ChartInfo, HelmAction, HelmChartNamespaces};
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::infrastructure::models::cloud_provider::service::{Action, Service};
use crate::runtime::block_on;

use crate::environment::action::deploy_external_secrets::{
    clean_unused_secrets_generated_by_eso, uninstall_service_external_secret,
};
use crate::environment::action::restart_service::RestartServiceAction;
use crate::environment::action::utils::{
    KubeObjectKind, delete_cached_image, delete_nlb_or_alb_service, get_last_deployed_image, mirror_image_if_necessary,
    validate_deployment_storage_replicas,
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
                KubeObjectKind::Deployment,
                target.environment.namespace(),
            ));

            Ok(TaskContext {
                last_deployed_image: last_image,
            })
        };

        let long_task = |_logger: &EnvProgressLogger, state: TaskContext| -> Result<TaskContext, Box<EngineError>> {
            // If the service have been paused, we must ensure we un-pause it first as hpa will not kick in
            let _ = PauseServiceAction::new(
                self.kube_label_selector(),
                false,
                Duration::from_secs(5 * 60),
                event_details.clone(),
                true,
            )
            .unpause_if_needed(target);

            if !self.storages.is_empty() {
                validate_deployment_storage_replicas(self.min_instances, self.max_instances, &event_details)?;
            }

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
                HelmChartValues::SerializedContext,
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

            // Clean unused secret generated by ESO
            let service_eso_secret_names: Vec<String> = self
                .external_secrets
                .iter()
                .map(|group| group.secret_name.to_string())
                .collect();
            clean_unused_secrets_generated_by_eso(
                target.kube.client(),
                target.environment.namespace(),
                &self.long_id,
                service_eso_secret_names,
            );
        };

        // At last we deploy our container
        execute_long_deployment(
            ApplicationDeploymentReporter::new_for_container(self, target, Action::Create),
            DeploymentTaskRef {
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
                    false,
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
                KubeObjectKind::Deployment,
                target.environment.namespace(),
            ));

            Ok(TaskContext {
                last_deployed_image: last_image,
            })
        };

        // Execute the deployment
        let long_task = |_logger: &EnvProgressLogger, state: TaskContext| -> Result<TaskContext, Box<EngineError>> {
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
                HelmChartValues::SerializedContext,
                chart,
            );

            helm.on_delete(target)?;

            Ok(state)
        };

        // Cleanup phase:
        // * the image from the cache
        // * the external secrets helm release if exists
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

            // Delete external secrets helm release if exists
            uninstall_service_external_secret(&self.kube_name, self.long_id(), target);
        };

        // Trigger deployment
        execute_long_deployment(
            ApplicationDeploymentReporter::new_for_container(self, target, Action::Delete),
            DeploymentTaskRef {
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
                    false,
                    self.get_event_details(Stage::Environment(EnvironmentStep::Restart)),
                );
                restart_service.on_restart(target)
            },
        )
    }
}

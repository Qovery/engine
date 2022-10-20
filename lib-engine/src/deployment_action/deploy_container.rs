use crate::build_platform::Image;
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
use crate::models::container::{Container, ContainerService, QOVERY_MIRROR_REPOSITORY_NAME};
use crate::models::types::{CloudProvider, ToTeraContext};
use crate::runtime::block_on;
use k8s_openapi::api::core::v1::PersistentVolumeClaim;

use crate::deployment_action::utils::{delete_cached_image, get_last_deployed_image, mirror_image, KubeObjectKind};
use std::path::PathBuf;
use std::time::Duration;

impl<T: CloudProvider> DeploymentAction for Container<T>
where
    Container<T>: ToTeraContext,
{
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        let logger = target.env_logger(self, EnvironmentStep::Deploy);

        mirror_image(
            &self.registry,
            &self.image,
            &self.tag,
            self.tag_for_mirror(),
            target,
            &logger,
            event_details.clone(),
        )?;

        // At last we deploy our container
        execute_long_deployment(
            ApplicationDeploymentReporter::new_for_container(self, target, Action::Create),
            || {
                // If the service have been paused, we must ensure we un-pause it first as hpa will not kick in
                let _ = PauseServiceAction::new(
                    self.selector(),
                    self.is_stateful(),
                    Duration::from_secs(5 * 60),
                    event_details.clone(),
                )
                .unpause_if_needed(target);

                let last_image = block_on(get_last_deployed_image(
                    target.kube.clone(),
                    &self.selector(),
                    if self.is_stateful() {
                        KubeObjectKind::Statefulset
                    } else {
                        KubeObjectKind::Deployment
                    },
                    target.environment.namespace(),
                ));

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
                    event_details.clone(),
                )?;

                // Delete previous image from cache to cleanup resources
                delete_cached_image(self.tag_for_mirror(), last_image, target, &logger)
                    .map_err(|err| EngineError::new_container_registry_error(event_details.clone(), err))?;

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
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Delete));

        execute_long_deployment(
            ApplicationDeploymentReporter::new_for_container(self, target, Action::Delete),
            || {
                let chart = ChartInfo {
                    name: self.helm_release_name(),
                    namespace: HelmChartNamespaces::Custom,
                    custom_namespace: Some(target.environment.namespace().to_string()),
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

                let logger = target.env_logger(self, EnvironmentStep::Delete);
                // Delete pvc of statefulset if needed
                // FIXME: Remove this after kubernetes 1.23 is deployed, at it should be done by kubernetes
                if self.is_stateful() {
                    logger.send_progress("🪓 Terminating network volume of the container".to_string());
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

                Ok(())
            },
        )?;

        let image = Image {
            name: QOVERY_MIRROR_REPOSITORY_NAME.to_string(),
            tag: self.tag_for_mirror(),
            registry_url: target.container_registry.registry_info().endpoint.clone(),
            repository_name: QOVERY_MIRROR_REPOSITORY_NAME.to_string(),
            ..Default::default()
        };

        let logger = target.env_logger(self, EnvironmentStep::Delete);
        logger.send_success("🪓 Deleting cached image of the container".to_string());
        target
            .container_registry
            .delete_image(&image)
            .map_err(|err| EngineError::new_container_registry_error(event_details, err))
    }
}

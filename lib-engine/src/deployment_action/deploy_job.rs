use crate::cloud_provider::helm::{ChartInfo, HelmChartNamespaces};
use crate::cloud_provider::service::{Action, Service};
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::command::CommandKiller;
use crate::cmd::docker::ContainerImage;
use crate::deployment_action::deploy_container::{get_last_deployed_image, get_url_with_credentials};
use crate::deployment_action::deploy_helm::HelmDeployment;
use crate::deployment_action::DeploymentAction;
use crate::deployment_report::execute_long_deployment;
use crate::deployment_report::job::reporter::JobDeploymentReporter;
use crate::errors::EngineError;
use crate::events::{EnvironmentStep, Stage};
use crate::models::container::QOVERY_MIRROR_REPOSITORY_NAME;
use crate::models::job::{Job, JobService};
use crate::models::types::{CloudProvider, ToTeraContext};
use crate::runtime::block_on;
use std::path::PathBuf;
use std::time::Duration;

impl<T: CloudProvider> DeploymentAction for Job<T>
where
    Job<T>: ToTeraContext,
{
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));

        let logger = target.env_logger(self, EnvironmentStep::Deploy);
        // We need to login to the registry to get access to the image
        let url = get_url_with_credentials(self.registry());
        if url.password().is_some() {
            logger.send_progress(format!(
                "🔓 Login to registry {} as user {}",
                url.host_str().unwrap_or_default(),
                url.username()
            ));
            if let Err(err) = target.docker.login(&url) {
                let err = EngineError::new_docker_error(event_details, err);
                let user_err = EngineError::new_engine_error(
                    err.clone(),
                    format!("❌ Failed to login to registry {}", url.host_str().unwrap_or_default()),
                    None,
                );
                logger.send_error(user_err);

                return Err(err);
            }
        }

        // Once we are logged to the registry, we mirror the user image into our cluster private registry
        // This is required only to avoid to manage rotating credentials
        logger.send_progress("🪞 Mirroring image to private cluster registry to ensure reproducibility".to_string());
        let registry_info = target.container_registry.registry_info();

        target
            .container_registry
            .create_repository(
                QOVERY_MIRROR_REPOSITORY_NAME,
                target.kubernetes.advanced_settings().registry_image_retention_time_sec,
            )
            .map_err(|err| EngineError::new_container_registry_error(event_details.clone(), err))?;

        let source_image = ContainerImage::new(self.registry.url().clone(), self.image.clone(), vec![self.tag.clone()]);
        let dest_image = ContainerImage::new(
            target.container_registry.registry_info().endpoint.clone(),
            (registry_info.get_image_name)(QOVERY_MIRROR_REPOSITORY_NAME),
            vec![self.tag_for_mirror()],
        );
        if let Err(err) = target.docker.mirror(
            &source_image,
            &dest_image,
            &mut |line| info!("{}", line),
            &mut |line| warn!("{}", line),
            &CommandKiller::from(Duration::from_secs(60 * 10), target.should_abort),
        ) {
            let err = EngineError::new_docker_error(event_details, err);
            let user_err = EngineError::new_engine_error(
                err.clone(),
                format!("❌ Failed to mirror image {}: {}", self.image_with_tag(), err),
                None,
            );
            logger.send_error(user_err);

            return Err(err);
        }

        execute_long_deployment(JobDeploymentReporter::new(self, target, Action::Create), || {
            let _last_image = block_on(get_last_deployed_image(
                target.kube.clone(),
                &self.selector(),
                false,
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

            helm.on_create(target)
        })
    }

    fn on_pause(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        unimplemented!()
    }
    fn on_delete(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        unimplemented!()
    }
}

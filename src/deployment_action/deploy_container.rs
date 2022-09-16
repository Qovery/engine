use crate::build_platform::Image;
use crate::cloud_provider::service::{delete_pending_service, Action, Service};
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::command::CommandKiller;
use crate::cmd::docker::ContainerImage;
use crate::container_registry::ecr::ECR;
use crate::deployment_action::deploy_helm::HelmDeployment;
use crate::deployment_action::pause_service::PauseServiceAction;
use crate::deployment_action::DeploymentAction;
use crate::deployment_report::application::reporter::ApplicationDeploymentReporter;
use crate::deployment_report::execute_long_deployment;
use crate::deployment_report::logger::get_loggers;
use crate::errors::{CommandError, EngineError};
use crate::events::{EnvironmentStep, Stage};
use crate::io_models::container::Registry;
use crate::kubers_utils::kube_delete_all_from_selector;
use crate::models::container::{Container, ContainerService};
use crate::models::types::{CloudProvider, ToTeraContext};
use crate::runtime::block_on;
use k8s_openapi::api::apps::v1::{Deployment, StatefulSet};
use k8s_openapi::api::core::v1::PersistentVolumeClaim;
use kube::api::ListParams;
use kube::Api;
use rusoto_core::{Client, HttpClient, Region};
use rusoto_credential::StaticProvider;
use rusoto_ecr::EcrClient;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;
use url::Url;

impl<T: CloudProvider> DeploymentAction for Container<T>
where
    Container<T>: ToTeraContext,
{
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));

        let loggers = get_loggers(self, *self.action());

        // We need to login to the registry to get access to the image
        let url = get_url_with_credentials(&self.registry);
        if url.password().is_some() {
            (loggers.send_progress)(format!(
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
                (loggers.send_error)(user_err);

                return Err(err);
            }
        }

        // Once we are logged to the registry, we mirror the user image into our cluster private registry
        // This is required only to avoid to manage rotating credentials
        (loggers.send_progress)("🪞 Mirroring image to private cluster registry to ensure reproducibility".to_string());
        let registry_info = target.container_registry.registry_info();

        target
            .container_registry
            .create_repository(
                Self::QOVERY_MIRROR_REPOSITORY_NAME,
                target.kubernetes.advanced_settings().registry_image_retention_time_sec,
            )
            .map_err(|err| EngineError::new_container_registry_error(event_details.clone(), err))?;

        let source_image = ContainerImage::new(self.registry.url().clone(), self.image.clone(), vec![self.tag.clone()]);
        let dest_image = ContainerImage::new(
            target.container_registry.registry_info().endpoint.clone(),
            (registry_info.get_image_name)(Self::QOVERY_MIRROR_REPOSITORY_NAME),
            vec![self.tag_for_mirror()],
        );
        if let Err(err) = target.docker.mirror(
            &source_image,
            &dest_image,
            &mut |line| info!("{}", line),
            &mut |line| warn!("{}", line),
            &CommandKiller::from_timeout(Duration::from_secs(60 * 10)),
        ) {
            let err = EngineError::new_docker_error(event_details, err);
            let user_err = EngineError::new_engine_error(
                err.clone(),
                format!("❌ Failed to mirror image {}: {}", self.image_with_tag(), err),
                None,
            );
            (loggers.send_error)(user_err);

            return Err(err);
        }

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
                    self.is_stateful(),
                    target.environment.namespace(),
                ));

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

                // Delete previous image from cache to cleanup resources
                if let Some(last_image_tag) = last_image.and_then(|img| img.split(':').last().map(str::to_string)) {
                    if last_image_tag != self.tag_for_mirror() {
                        let logger = get_loggers(self, Action::Create);
                        (logger.send_progress)(format!("🪓 Deleting previous cached image {}", last_image_tag));

                        let image = Image {
                            name: Self::QOVERY_MIRROR_REPOSITORY_NAME.to_string(),
                            tag: last_image_tag,
                            registry_url: target.container_registry.registry_info().endpoint.clone(),
                            repository_name: Self::QOVERY_MIRROR_REPOSITORY_NAME.to_string(),
                            ..Default::default()
                        };

                        target
                            .container_registry
                            .delete_image(&image)
                            .map_err(|err| EngineError::new_container_registry_error(event_details.clone(), err))?;
                    }
                }

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
                let helm = HelmDeployment::new(
                    self.helm_release_name(),
                    self.to_tera_context(target)?,
                    PathBuf::from(self.helm_chart_dir()),
                    PathBuf::from(self.workspace_directory()),
                    self.get_event_details(Stage::Environment(EnvironmentStep::Delete)),
                    Some(self.selector()),
                    None,
                );

                helm.on_delete(target)?;

                let logger = get_loggers(self, Action::Delete);
                // Delete pvc of statefulset if needed
                // FIXME: Remove this after kubernetes 1.23 is deployed, at it should be done by kubernetes
                if self.is_stateful() {
                    (logger.send_progress)("🪓 Terminating network volume of the container".to_string());
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
            name: Self::QOVERY_MIRROR_REPOSITORY_NAME.to_string(),
            tag: self.tag_for_mirror(),
            registry_url: target.container_registry.registry_info().endpoint.clone(),
            repository_name: Self::QOVERY_MIRROR_REPOSITORY_NAME.to_string(),
            ..Default::default()
        };

        let logger = get_loggers(self, Action::Delete);
        (logger.send_progress)("🪓 Deleting cached image of the container".to_string());
        target
            .container_registry
            .delete_image(&image)
            .map_err(|err| EngineError::new_container_registry_error(event_details, err))
    }
}

fn get_url_with_credentials(registry: &Registry) -> Url {
    let url = match registry {
        Registry::DockerHub { url, credentials, .. } => {
            let mut url = url.clone();
            if let Some(credentials) = credentials {
                let _ = url.set_username(&credentials.login);
                let _ = url.set_password(Some(&credentials.password));
            }
            url
        }
        Registry::DoCr { url, token, .. } => {
            let mut url = url.clone();
            let _ = url.set_username(token);
            let _ = url.set_password(Some(token));
            url
        }
        Registry::ScalewayCr {
            url,
            scaleway_access_key: _,
            scaleway_secret_key,
            ..
        } => {
            let mut url = url.clone();
            let _ = url.set_username("nologin");
            let _ = url.set_password(Some(scaleway_secret_key));
            url
        }
        Registry::PrivateEcr {
            url: _,
            region,
            access_key_id,
            secret_access_key,
            ..
        } => {
            let creds = StaticProvider::new(access_key_id.to_string(), secret_access_key.to_string(), None, None);
            let region = Region::from_str(region).unwrap_or_default();
            let ecr_client = EcrClient::new_with_client(Client::new_with(creds, HttpClient::new().unwrap()), region);

            let credentials = ECR::get_credentials(&ecr_client).unwrap();
            let mut url = Url::parse(credentials.endpoint_url.as_str()).unwrap();
            let _ = url.set_username(&credentials.access_token);
            let _ = url.set_password(Some(&credentials.password));
            url
        }
        Registry::PublicEcr { url, .. } => url.clone(),
    };

    url
}

async fn get_last_deployed_image(
    client: kube::Client,
    selector: &str,
    is_statefulset: bool,
    namespace: &str,
) -> Option<String> {
    let list_params = ListParams::default().labels(selector);

    if is_statefulset {
        let api: Api<StatefulSet> = Api::namespaced(client, namespace);
        Some(
            api.list(&list_params)
                .await
                .ok()?
                .items
                .first()?
                .spec
                .as_ref()?
                .template
                .spec
                .as_ref()?
                .containers
                .first()?
                .image
                .as_ref()?
                .to_string(),
        )
    } else {
        let api: Api<Deployment> = Api::namespaced(client, namespace);
        Some(
            api.list(&list_params)
                .await
                .ok()?
                .items
                .first()?
                .spec
                .as_ref()?
                .template
                .spec
                .as_ref()?
                .containers
                .first()?
                .image
                .as_ref()?
                .to_string(),
        )
    }
}

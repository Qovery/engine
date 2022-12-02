use crate::build_platform::Image;
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::command::CommandKiller;
use crate::cmd::docker::ContainerImage;
use crate::container_registry::errors::ContainerRegistryError;
use crate::deployment_report::logger::{EnvProgressLogger, EnvSuccessLogger};
use crate::errors::EngineError;
use crate::events::EventDetails;
use crate::io_models::container::Registry;
use crate::models::container::QOVERY_MIRROR_REPOSITORY_NAME;
use k8s_openapi::api::apps::v1::{Deployment, StatefulSet};
use k8s_openapi::api::batch::v1::CronJob;
use kube::api::ListParams;
use kube::Api;
use std::time::Duration;

pub fn delete_cached_image(
    current_image_tag: String,
    last_image: Option<String>,
    force_delete: bool,
    target: &DeploymentTarget,
    logger: &EnvSuccessLogger,
) -> Result<(), ContainerRegistryError> {
    // Delete previous image from cache to cleanup resources
    if let Some(last_image_tag) = last_image.and_then(|img| img.split(':').last().map(str::to_string)) {
        if force_delete || last_image_tag != current_image_tag {
            logger.send_success(format!("🪓 Deleting previous cached image {}", last_image_tag));

            let image = Image {
                name: QOVERY_MIRROR_REPOSITORY_NAME.to_string(),
                tag: last_image_tag,
                registry_url: target.container_registry.registry_info().endpoint.clone(),
                repository_name: QOVERY_MIRROR_REPOSITORY_NAME.to_string(),
                ..Default::default()
            };

            target.container_registry.delete_image(&image)?;
        }
    }

    Ok(())
}

pub fn mirror_image(
    registry: &Registry,
    image_name: &str,
    tag: &str,
    tag_for_mirror: String,
    target: &DeploymentTarget,
    logger: &EnvProgressLogger,
    event_details: EventDetails,
) -> Result<(), EngineError> {
    // We need to login to the registry to get access to the image
    let url = registry.get_url_with_credentials();
    if url.password().is_some() {
        logger.info(format!(
            "🔓 Login to registry {} as user {}",
            url.host_str().unwrap_or_default(),
            url.username()
        ));
        if let Err(err) = target.docker.login(&url) {
            let err = EngineError::new_docker_error(event_details, err);
            let user_err = EngineError::new_engine_error(
                err,
                format!("❌ Failed to login to registry {}", url.host_str().unwrap_or_default()),
                None,
            );
            return Err(user_err);
        }
    }

    // Once we are logged to the registry, we mirror the user image into our cluster private registry
    // This is required only to avoid to manage rotating credentials
    logger.info("🪞 Mirroring image to private cluster registry to ensure reproducibility".to_string());
    let registry_info = target.container_registry.registry_info();

    target
        .container_registry
        .create_repository(
            QOVERY_MIRROR_REPOSITORY_NAME,
            target.kubernetes.advanced_settings().registry_image_retention_time_sec,
        )
        .map_err(|err| EngineError::new_container_registry_error(event_details.clone(), err))?;

    let source_image = ContainerImage::new(registry.url().clone(), image_name.to_string(), vec![tag.to_string()]);
    let dest_image = ContainerImage::new(
        target.container_registry.registry_info().endpoint.clone(),
        (registry_info.get_image_name)(QOVERY_MIRROR_REPOSITORY_NAME),
        vec![tag_for_mirror],
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
            format!("❌ Failed to mirror image {}/{}: {}", image_name, tag, err),
            None,
        );

        return Err(user_err);
    }
    Ok(())
}

pub enum KubeObjectKind {
    Deployment,
    Statefulset,
    Job,
    CronJob,
}

pub async fn get_last_deployed_image(
    client: kube::Client,
    selector: &str,
    service_type: KubeObjectKind,
    namespace: &str,
) -> Option<String> {
    let list_params = ListParams::default().labels(selector);

    match service_type {
        KubeObjectKind::Deployment => {
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
        KubeObjectKind::Statefulset => {
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
        }
        KubeObjectKind::CronJob => {
            let api: Api<CronJob> = Api::namespaced(client, namespace);
            Some(
                api.list(&list_params)
                    .await
                    .ok()?
                    .items
                    .first()?
                    .spec
                    .as_ref()?
                    .job_template
                    .spec
                    .as_ref()?
                    .template
                    .spec
                    .as_ref()?
                    .containers
                    .last()?
                    .image
                    .as_ref()?
                    .to_string(),
            )
        }
        KubeObjectKind::Job => {
            let api: Api<k8s_openapi::api::batch::v1::Job> = Api::namespaced(client, namespace);
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
                    .last()? // last because of busybox container that wait on output
                    .image
                    .as_ref()?
                    .to_string(),
            )
        }
    }
}

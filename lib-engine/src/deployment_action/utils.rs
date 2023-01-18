use crate::build_platform::Image;
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::command::CommandKiller;
use crate::cmd::docker::ContainerImage;
use crate::container_registry::errors::ContainerRegistryError;
use crate::deployment_report::logger::{EnvProgressLogger, EnvSuccessLogger};
use crate::errors::EngineError;
use crate::events::EventDetails;
use crate::io_models::container::Registry;
use crate::kubers_utils::kube_get_resources_by_selector;
use crate::models::container::get_mirror_repository_name;
use crate::runtime::block_on;
use k8s_openapi::api::apps::v1::{Deployment, StatefulSet};
use k8s_openapi::api::batch::v1::CronJob;
use k8s_openapi::api::core::v1::Service;
use kube::api::ListParams;
use kube::Api;
use std::time::Duration;
use uuid::Uuid;

pub fn delete_cached_image(
    service_id: &Uuid,
    current_image_tag: String,
    last_image: Option<String>,
    is_service_deletion: bool,
    target: &DeploymentTarget,
    logger: &EnvSuccessLogger,
) -> Result<(), ContainerRegistryError> {
    // Delete previous image from cache to cleanup resources
    if let Some(last_image_tag) = last_image.and_then(|img| img.split(':').last().map(str::to_string)) {
        if is_service_deletion || last_image_tag != current_image_tag {
            logger.send_success(format!("🪓 Deleting previous cached image {}", last_image_tag));

            let mirror_repo_name = get_mirror_repository_name(service_id);
            let image = Image {
                name: mirror_repo_name.clone(),
                tag: last_image_tag,
                registry_url: target.container_registry.registry_info().endpoint.clone(),
                repository_name: mirror_repo_name.clone(),
                ..Default::default()
            };

            target.container_registry.delete_image(&image)?;
            if is_service_deletion {
                target.container_registry.delete_repository(&mirror_repo_name)?;
            }
        }
    }

    Ok(())
}

pub fn mirror_image(
    service_id: &Uuid,
    registry: &Registry,
    image_name: &str,
    tag: &str,
    tag_for_mirror: String,
    target: &DeploymentTarget,
    logger: &EnvProgressLogger,
    event_details: EventDetails,
) -> Result<(), Box<EngineError>> {
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
            return Err(Box::new(user_err));
        }
    }

    // Once we are logged to the registry, we mirror the user image into our cluster private registry
    // This is required only to avoid to manage rotating credentials
    logger.info("🪞 Mirroring image to private cluster registry to ensure reproducibility".to_string());
    let registry_info = target.container_registry.registry_info();

    let mirror_repo_name = get_mirror_repository_name(service_id);
    target
        .container_registry
        .create_repository(
            mirror_repo_name.as_str(),
            target.kubernetes.advanced_settings().registry_image_retention_time_sec,
        )
        .map_err(|err| EngineError::new_container_registry_error(event_details.clone(), err))?;

    let source_image = ContainerImage::new(registry.url().clone(), image_name.to_string(), vec![tag.to_string()]);
    let dest_image = ContainerImage::new(
        target.container_registry.registry_info().endpoint.clone(),
        (registry_info.get_image_name)(&mirror_repo_name),
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

        return Err(Box::new(user_err));
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

pub fn k8s_external_service_name_exists(
    client: &kube::Client,
    namespace: &str,
    selector: &str,
    event_details: &EventDetails,
    service_id: &str,
) -> Result<bool, Box<EngineError>> {
    let result = match block_on(kube_get_resources_by_selector::<Service>(client, namespace, selector)) {
        Err(e) => {
            return Err(Box::new(EngineError::new_k8s_cannot_get_services(
                event_details.clone(),
                e,
                service_id,
            )))
        }
        Ok(result) => result.items,
    };

    let svc = result.first();

    match svc {
        None => Ok(false),
        Some(svc) => {
            if let Some(spec) = &svc.spec {
                if let Some(type_) = &spec.type_ {
                    return match type_.to_lowercase() == "externalname" {
                        true => Ok(true),
                        false => Ok(false),
                    };
                }
                return Ok(false);
            }
            Ok(false)
        }
    }
}

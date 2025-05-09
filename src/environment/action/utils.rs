use crate::cmd::command::CommandKiller;
use crate::cmd::docker::ContainerImage;
use crate::environment::report::logger::EnvProgressLogger;
use crate::errors::{CommandError, EngineError};
use crate::events::EventDetails;
use crate::infrastructure::models::build_platform::Image;
use crate::infrastructure::models::cloud_provider::DeploymentTarget;
use crate::infrastructure::models::cloud_provider::io::RegistryMirroringMode;
use crate::infrastructure::models::container_registry::RegistryTags;
use crate::infrastructure::models::container_registry::errors::ContainerRegistryError;

use crate::environment::models::container::get_mirror_repository_name;
use crate::environment::models::kubernetes::K8sObject;
use crate::environment::models::registry_image_source::RegistryImageSource;
use crate::metrics_registry::{MetricsRegistry, StepLabel, StepName, StepStatus};
use crate::runtime::block_on;
use crate::services::kube_client::{QubeClient, SelectK8sResourceBy};

use k8s_openapi::api::apps::v1::{Deployment, StatefulSet};
use k8s_openapi::api::batch::v1::CronJob;

use crate::infrastructure::models::cloud_provider::service::{Service, increase_storage_size};
use crate::io_models::models::InvalidStatefulsetStorage;
use crate::kubers_utils::kube_get_resources_by_selector;
use k8s_openapi::api::core::v1::PersistentVolumeClaim;
use kube::Api;
use kube::api::ListParams;
use retry::OperationResult;
use retry::delay::{Fibonacci, Fixed};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use uuid::Uuid;

// specific to AWS
pub fn delete_nlb_or_alb_service(
    qube_client: QubeClient,
    namespace: &str,
    label_selector: &str,
    aws_eks_enable_alb_controller: bool,
    event_details: EventDetails,
) -> Result<(), Box<EngineError>> {
    let services = block_on(qube_client.get_services(
        event_details.clone(),
        Some(namespace),
        SelectK8sResourceBy::LabelsSelector(label_selector.to_string()),
    ))?;

    // annotations corresponding to service to delete if found (to be later replaced)
    let service_nlb_annotation_to_delete = match aws_eks_enable_alb_controller {
        true => "nlb".to_string(),       // without ALB controller
        false => "external".to_string(), // with ALB controller
    };

    // search for nlb annotation
    let mut deleted_nlb = false;
    for service in &services {
        if service.get_annotation_value("service.beta.kubernetes.io/aws-load-balancer-type")
            == Some(&service_nlb_annotation_to_delete)
        {
            warn!("Deleting service {} with NLB annotation", service.metadata.name.as_str());
            block_on(qube_client.delete_service_from_name(
                event_details.clone(),
                namespace,
                service.metadata.name.as_str(),
            ))?;
            deleted_nlb = true;
            break;
        }
    }

    // wait for the NLB to be deleted before continuing
    if deleted_nlb {
        // error message if timeout waiting for NLB to be deleted
        let msg = format!(
            "Failed to delete NLB service in namespace '{}' with selector '{}', timed out. Please retry to deploy later or look at AWS Cloudwatch issue.",
            namespace, service_nlb_annotation_to_delete
        );
        let err = EngineError::new_k8s_delete_service_error(
            event_details.clone(),
            CommandError::new_from_safe_message(msg.clone()),
            msg,
        );

        let result = retry::retry(Fixed::from_millis(5 * 1000).take(32), || {
            let services = match block_on(qube_client.get_services(
                event_details.clone(),
                Some(namespace),
                SelectK8sResourceBy::LabelsSelector(label_selector.to_string()),
            )) {
                Ok(x) => x,
                Err(e) => return OperationResult::Retry(e),
            };

            for service in services {
                if service.get_annotation_value("service.beta.kubernetes.io/aws-load-balancer-type")
                    == Some(&service_nlb_annotation_to_delete)
                {
                    info!(
                        "Waiting for NLB service {}/{} to be deleted...",
                        service.metadata.namespace, service.metadata.name
                    );
                    return OperationResult::Retry(Box::new(err.clone()));
                }
            }
            OperationResult::Ok(())
        });
        match result {
            Ok(_) => (),
            Err(retry::Error { error, .. }) => return Err(error),
        }
    }

    Ok(())
}

pub fn delete_cached_image(
    service_id: &Uuid,
    current_image_tag: String,
    last_image: Option<String>,
    is_service_deletion: bool,
    target: &DeploymentTarget,
    logger: &dyn Fn(String),
) -> Result<(), ContainerRegistryError> {
    if target.kubernetes.advanced_settings().registry_mirroring_mode == RegistryMirroringMode::Cluster {
        // Do no delete image when mirroring mode is Cluster because it can be used by another service
        return Ok(());
    }

    // Delete previous image from cache to cleanup resources
    if let Some(last_image_tag) = last_image
        .as_ref()
        .and_then(|img| img.split(':').last().map(str::to_string))
    {
        if is_service_deletion || last_image_tag != current_image_tag {
            logger(format!("🪓 Deleting previous cached image {last_image_tag}"));
            let mirror_repo_name = get_mirror_repository_name(
                service_id,
                target.kubernetes.long_id(),
                &target.kubernetes.advanced_settings().registry_mirroring_mode,
            );
            let mirror_repo_name = target
                .container_registry
                .registry_info()
                .get_repository_name(&mirror_repo_name);
            let image = Image {
                name: mirror_repo_name.clone(),
                tag: last_image_tag,
                registry_url: target
                    .container_registry
                    .get_registry_endpoint(Some(target.kubernetes.cluster_name().as_str())),
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

pub fn mirror_image_if_necessary(
    service_id: &Uuid,
    source: &RegistryImageSource,
    target: &DeploymentTarget,
    logger: &EnvProgressLogger,
    event_details: EventDetails,
    metrics_registry: Arc<dyn MetricsRegistry>,
) -> Result<(), Box<EngineError>> {
    let mirror_record = metrics_registry.start_record(*service_id, StepLabel::Service, StepName::MirrorImage);

    let (cluster_container_registry, image_name, image_tag, must_mirror_image) = source
        .compute_cluster_container_registry_url_with_image_name_and_image_tag(
            service_id,
            target.kubernetes.long_id(),
            &target.kubernetes.advanced_settings().registry_mirroring_mode,
            target.container_registry.registry_info(),
        );
    let dest_image = ContainerImage::new(cluster_container_registry, image_name, vec![image_tag]);

    if image_already_exist(&dest_image, target) {
        let skip_image_mirroring_message = if must_mirror_image {
            format!(
                "🎯 Skipping image mirroring: image {} already exists in the registry",
                source.image
            )
        } else {
            "🎯 Skipping image mirroring: service and cluster registries are the same".to_string()
        };
        logger.info(skip_image_mirroring_message);
        mirror_record.stop(StepStatus::Skip);
        Ok(())
    } else {
        let result = mirror_image(
            service_id,
            source,
            &dest_image,
            target,
            logger,
            event_details.clone(),
            RegistryTags {
                cluster_id: Some(target.kubernetes.cluster_name()),
                environment_id: Some(target.environment.id.clone()),
                project_id: Some(target.environment.project_id.clone()),
                resource_ttl: target.kubernetes.advanced_settings().resource_ttl(),
            },
        );
        mirror_record.stop(if result.is_ok() {
            StepStatus::Success
        } else {
            StepStatus::Error
        });
        result
    }
}

fn image_already_exist(dest_image: &ContainerImage, target: &DeploymentTarget) -> bool {
    matches!(target.docker.does_image_exist_remotely(dest_image), Ok(true))
}

fn mirror_image(
    service_id: &Uuid,
    source: &RegistryImageSource,
    dest_image: &ContainerImage,
    target: &DeploymentTarget,
    logger: &EnvProgressLogger,
    event_details: EventDetails,
    tags: RegistryTags,
) -> Result<(), Box<EngineError>> {
    // We need to login to the registry to get access to the image
    let url = source.registry.get_url_with_credentials().map_err(|_| {
        logger.warning("⚠️Cannot get the registry credentials".to_string());
        EngineError::new_error_cannot_get_registry_credentials(event_details.clone())
    })?;
    if url.password().is_some() {
        logger.info(format!(
            "🔓 Login to registry {} as user {}",
            url.host_str().unwrap_or_default(),
            url.username()
        ));

        let login_ret = retry::retry(Fibonacci::from(Duration::from_secs(1)).take(4), || {
            target.docker.login(&url).inspect_err(|_err| {
                logger.warning("🔓 Retrying to login to registry due to error...".to_string());
            })
        });

        if let Err(err) = login_ret {
            let err = EngineError::new_docker_error(event_details, err.error);
            let msg = format!(
                "❌ Failed to login to registry {} due to {}",
                url.host_str().unwrap_or_default(),
                err
            );
            let user_err = EngineError::new_engine_error(err, msg, None);
            return Err(Box::new(user_err));
        }
    }

    // Once we are logged to the registry, we mirror the user image into our cluster private registry
    // This is required only to avoid to manage rotating credentials
    logger.info("🪞 Mirroring image to private cluster registry to ensure reproducibility".to_string());
    let mirror_repo_name = get_mirror_repository_name(
        service_id,
        target.kubernetes.long_id(),
        &target.kubernetes.advanced_settings().registry_mirroring_mode,
    );
    target
        .container_registry
        .create_repository(
            Some(target.kubernetes.cluster_name().as_str()),
            mirror_repo_name.as_str(),
            target.kubernetes.advanced_settings().registry_image_retention_time_sec,
            tags,
        )
        .map_err(|err| EngineError::new_container_registry_error(event_details.clone(), err))?;

    let source_image = ContainerImage::new(
        source.registry.url().clone(),
        source.image.to_string(),
        vec![source.tag.to_string()],
    );

    let should_abort_waiting_thread = AtomicBool::new(false);
    let current_span = tracing::Span::current();
    let result = thread::scope(|scope| {
        let waiting_thread = scope.spawn(|| {
            // making sure to pass the current span to the new thread not to lose any tracing info
            let _span = current_span.enter();
            let mut iterations: u16 = 0;
            loop {
                thread::sleep(Duration::from_secs(1));
                if should_abort_waiting_thread.load(Ordering::Relaxed) {
                    break;
                }
                iterations += 1;
                // Send a message every minute to reassure user
                if iterations % 60 == 0 {
                    logger.info("🪞 Mirroring is still in progress...".to_string());
                    iterations = 0;
                }
            }
        });
        let docker_mirror_thread = scope.spawn(|| {
            // making sure to pass the current span to the new thread not to lose any tracing info
            let _span = current_span.enter();
            let mut err_logs = Vec::new();
            if let Err(err) = retry::retry(Fixed::from_millis(1000).take(3), || {
                err_logs.clear();
                match target.docker.mirror(
                    &source_image,
                    dest_image,
                    &mut |line| info!("{}", line),
                    &mut |line| {
                        warn!("{}", &line);
                        err_logs.push(line);
                    },
                    // Set timeout at 30min (arbitrary value, but some big images >= 8 Go takes more than 15 minutes to be pulled)
                    &CommandKiller::from(Duration::from_secs(60 * 30), target.abort),
                ) {
                    Ok(ret) => OperationResult::Ok(ret),
                    Err(err) if err.is_aborted() => OperationResult::Err(err),
                    Err(err) => {
                        error!("docker mirror error: {:?}", err);
                        logger.info("🪞 Retrying Mirroring image due to error...".to_string());
                        OperationResult::Retry(err)
                    }
                }
            }) {
                let msg = format!(
                    "❌ Failed to mirror image {}:{} due to {}\n{}",
                    source.image,
                    source.tag,
                    err,
                    err_logs.join("\n")
                );
                logger.warning(msg.clone());
                let user_err = EngineError::new_docker_error(event_details, err.error);
                return Err(Box::new(EngineError::new_engine_error(user_err, msg, None)));
            }

            Ok(())
        });

        // Wait docker mirror thread to finish
        let result = docker_mirror_thread.join().unwrap();

        // Release waiting thread
        should_abort_waiting_thread.store(true, Ordering::Relaxed);
        waiting_thread.join().unwrap();

        // Return docker mirroring thread result
        result
    });

    result
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

pub fn are_pvcs_bound(
    service: &dyn Service,
    namespace: &str,
    event_details: &EventDetails,
    kube_client: &kube::Client,
) -> Result<(), Box<EngineError>> {
    let selector = service.kube_label_selector();
    match block_on(kube_get_resources_by_selector::<PersistentVolumeClaim>(
        kube_client,
        namespace,
        &selector,
    )) {
        Ok(pvcs) => {
            for pvc in pvcs.items {
                if let (Some(status), Some(name)) = (pvc.status, pvc.metadata.name) {
                    if let Some(phase) = status.phase {
                        if phase.to_lowercase().as_str() != "bound" {
                            return Err(Box::new(EngineError::new_k8s_cannot_bound_pvc(
                                event_details.clone(),
                                CommandError::new_from_safe_message(format!("Can't bound PVC {name}")),
                                service.name(),
                            )));
                        };
                    }
                }
            }

            Ok(())
        }
        Err(e) => Err(Box::new(EngineError::new_k8s_enable_to_get_pvc(event_details.clone(), e))),
    }
}

pub fn update_pvcs(
    service: &dyn Service,
    invalid_statefulset: &InvalidStatefulsetStorage,
    namespace: &str,
    event_details: &EventDetails,
    client: &kube::Client,
) -> Result<(), Box<EngineError>> {
    block_on(increase_storage_size(namespace, invalid_statefulset, event_details, client))?;

    are_pvcs_bound(service, namespace, event_details, client)?;

    Ok(())
}

use crate::cloud_provider::kubernetes::Kubernetes;
use crate::errors::{CommandError, EngineError};
use crate::events::Stage::Infrastructure;
use crate::events::{EngineEvent, EventDetails, EventMessage, InfrastructureStep};
use crate::object_storage::ObjectStorage;
use crate::utilities::to_short_id;
use retry::delay::Fibonacci;
use retry::OperationResult;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use uuid::Uuid;

fn get_kubeconfig_filename(cluster_id: &Uuid) -> String {
    format!("{}.yaml", to_short_id(cluster_id))
}

fn get_bucket_name(cluster_id: &Uuid) -> String {
    format!("qovery-kubeconfigs-{}", to_short_id(cluster_id))
}

pub fn put_kubeconfig_file_to_object_storage(
    kube: &dyn Kubernetes,
    object_store: &dyn ObjectStorage,
) -> Result<(), Box<EngineError>> {
    if let Err(e) = object_store.put_object(
        get_bucket_name(kube.long_id()).as_str(),
        get_kubeconfig_filename(kube.long_id()).as_str(),
        &kube.kubeconfig_local_file_path(),
        None,
    ) {
        let event_details = kube.get_event_details(Infrastructure(InfrastructureStep::LoadConfiguration));
        return Err(Box::new(EngineError::new_object_storage_error(event_details, e)));
    };

    let kubeconfig = fs::read_to_string(kube.kubeconfig_local_file_path()).unwrap_or_default();
    write_kubeconfig_on_disk(
        &kube.kubeconfig_local_file_path(),
        &kubeconfig,
        kube.get_event_details(Infrastructure(InfrastructureStep::LoadConfiguration)),
    )?;

    // Upload kubeconfig, so we can store it in the core
    if let Err(err) = kube.context().qovery_api.update_cluster_credentials(kubeconfig) {
        error!("Cannot update cluster credentials {}", err);
    }

    Ok(())
}

pub fn delete_kubeconfig_from_object_storage(
    kube: &dyn Kubernetes,
    object_store: &dyn ObjectStorage,
) -> Result<(), Box<EngineError>> {
    if let Err(e) = object_store.delete_object(
        get_bucket_name(kube.long_id()).as_str(),
        get_kubeconfig_filename(kube.long_id()).as_str(),
    ) {
        let event_details = kube.get_event_details(Infrastructure(InfrastructureStep::LoadConfiguration));
        return Err(Box::new(EngineError::new_object_storage_error(event_details, e)));
    };

    Ok(())
}

pub fn write_kubeconfig_on_disk(
    kubeconfig_path: &Path,
    kubeconfig: &str,
    event_details: EventDetails,
) -> Result<(), Box<EngineError>> {
    fs::create_dir_all(
        kubeconfig_path
            .parent()
            .expect("Couldn't create kubeconfig folder parent path"),
    )
    .map_err(|err| EngineError::new_cannot_create_file(event_details.clone(), err.into()))?;

    let mut file = File::create(kubeconfig_path)
        .map_err(|err| EngineError::new_cannot_create_file(event_details.clone(), err.into()))?;
    file.write_all(kubeconfig.as_bytes())
        .map_err(|err| EngineError::new_cannot_write_file(event_details.clone(), err.into()))?;

    let metadata = match file.metadata() {
        Ok(metadata) => metadata,
        Err(err) => {
            let error = EngineError::new_cannot_retrieve_cluster_config_file(
                event_details.clone(),
                CommandError::new("Error getting file metadata.".to_string(), Some(err.to_string()), None),
            );
            return Err(Box::new(error));
        }
    };

    let max_size = 16 * 1024;
    if metadata.len() > max_size {
        return Err(Box::new(EngineError::new_kubeconfig_size_security_check_error(
            event_details.clone(),
            metadata.len(),
            max_size,
        )));
    };

    let mut permissions = metadata.permissions();
    permissions.set_mode(0o600);
    if let Err(err) = file.set_permissions(permissions) {
        let error = EngineError::new_cannot_retrieve_cluster_config_file(
            event_details.clone(),
            CommandError::new("Error getting file permissions.".to_string(), Some(err.to_string()), None),
        );
        return Err(Box::new(error));
    }

    Ok(())
}

pub fn force_fetch_kubeconfig(kube: &dyn Kubernetes, object_store: &dyn ObjectStorage) -> Result<(), Box<EngineError>> {
    let event_details = kube.get_event_details(Infrastructure(InfrastructureStep::LoadConfiguration));
    let stage = Infrastructure(InfrastructureStep::RetrieveClusterConfig);

    let object_key = get_kubeconfig_filename(kube.long_id());
    let bucket_name = get_bucket_name(kube.long_id());
    match retry::retry(Fibonacci::from_millis(5000).take(5), || {
        match object_store.get_object(bucket_name.as_str(), object_key.as_str()) {
            Ok(bucket_object) => {
                let file_path = kube.kubeconfig_local_file_path();
                let kubeconfig = String::from_utf8_lossy(&bucket_object.value);
                if let Err(err) = write_kubeconfig_on_disk(
                    &file_path,
                    &kubeconfig,
                    kube.get_event_details(Infrastructure(InfrastructureStep::LoadConfiguration)),
                ) {
                    return OperationResult::Retry(err);
                }

                // Upload kubeconfig, so we can store it
                if let Err(err) = kube
                    .context()
                    .qovery_api
                    .update_cluster_credentials(kubeconfig.to_string())
                {
                    error!("Cannot update cluster credentials {}", err);
                }

                OperationResult::Ok(())
            }
            Err(err) => {
                let error = EngineError::new_cannot_retrieve_cluster_config_file(
                    kube.get_event_details(stage.clone()),
                    err.into(),
                );

                OperationResult::Retry(Box::new(error))
            }
        }
    }) {
        Ok(_) => (),
        Err(retry::Error { error, .. }) => {
            kube.logger().log(EngineEvent::Info(
                event_details,
                EventMessage::new(
                    "Cannot retrieve kubeconfig from previous installation.".to_string(),
                    Some(error.to_string()),
                ),
            ));

            return Err(error);
        }
    };

    Ok(())
}

pub fn fetch_kubeconfig(kube: &dyn Kubernetes, object_store: &dyn ObjectStorage) -> Result<(), Box<EngineError>> {
    if kube.context().is_first_cluster_deployment() {
        return Ok(());
    }

    force_fetch_kubeconfig(kube, object_store)
}

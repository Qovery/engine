use crate::cloud_provider::models::InvalidPVCStorage;
use crate::errors::CommandError;
use k8s_openapi::api::apps::v1::StatefulSet;
use k8s_openapi::api::core::v1::PersistentVolumeClaim;
use kube::api::{DeleteParams, ListParams, ObjectList, Patch, PatchParams, PostParams};
use kube::{Api, Resource};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fmt::Debug;

pub enum KubeDeleteMode {
    Normal,
    Orphan,
}

pub async fn kube_delete_all_from_selector<K>(
    client: &kube::Client,
    selector: &str,
    namespace: &str,
    delete_mode: KubeDeleteMode,
) -> Result<(), kube::Error>
where
    K: Clone + DeserializeOwned + Debug + Resource,
    <K as Resource>::DynamicType: Default,
{
    let obj_name = K::kind(&K::DynamicType::default()).to_string();
    info!("Deleting k8s {} from selector {}", obj_name, selector);

    let list_params = ListParams::default().labels(selector);
    let delete_params = match delete_mode {
        KubeDeleteMode::Normal => DeleteParams::background(),
        KubeDeleteMode::Orphan => DeleteParams::orphan(),
    };

    let api: Api<K> = Api::namespaced(client.clone(), namespace);
    let ret = api.delete_collection(&delete_params, &list_params).await?;

    info!("Deletion of k8s {} matching {} returned {:?}", obj_name, selector, ret);

    Ok(())
}

pub async fn kube_edit_pvc_size(
    client: &kube::Client,
    namespace: &str,
    invalid_pvc: &InvalidPVCStorage,
) -> Result<(), CommandError> {
    let obj_name = "PersistentVolumeClaim";
    info!("Updating k8s {} from name {}", obj_name, invalid_pvc.pvc_name);

    let api: Api<PersistentVolumeClaim> = Api::namespaced(client.clone(), namespace);
    let desired_size = format!("{}Gi", invalid_pvc.required_disk_size_in_gib);
    let patch = serde_json::json!({
        "apiVersion": "v1",
        "kind": "PersistentVolumeClaim",
        "metadata": {
            "name": invalid_pvc.pvc_name,
        },
        "spec": {
            "accessModes":
            ["ReadWriteOnce"],
            "resources": {
                "requests": {
                    "storage": desired_size,
                }
            }
        }
    });
    let mut params = PatchParams::apply("qovery");
    params.force = true;
    let patch = Patch::Apply(&patch);
    api.patch(&invalid_pvc.pvc_name, &params, &patch).await.map_err(|e| {
        CommandError::new(format!("Unable to update pvc {} size.", obj_name), Some(e.to_string()), None)
    })?;
    Ok(())
}

pub async fn kube_get_resources_by_selector<K>(
    client: &kube::Client,
    namespace: &str,
    selector: &str,
) -> Result<ObjectList<K>, CommandError>
where
    K: Clone + DeserializeOwned + Debug + Resource,
    <K as Resource>::DynamicType: Default,
{
    let obj_name = K::kind(&K::DynamicType::default()).to_string();
    info!("Getting k8s {} from selector {}", obj_name, selector);

    let api: Api<K> = Api::namespaced(client.clone(), namespace);
    let params = ListParams::default().labels(selector);
    let resources = api.list(&params).await.map_err(|e| {
        CommandError::new(
            format!("Unable to get {} with selector {}.", obj_name, selector),
            Some(e.to_string()),
            None,
        )
    })?;

    Ok(resources)
}

pub async fn kube_create_from_resource<K>(
    client: &kube::Client,
    namespace: &str,
    resource: K,
) -> Result<(), CommandError>
where
    K: Clone + DeserializeOwned + Debug + Resource + Serialize,
    <K as Resource>::DynamicType: Default,
{
    let obj_name = K::kind(&K::DynamicType::default()).to_string();
    info!("Creating k8s {} in {}", obj_name, namespace);

    let post_params = PostParams::default();
    let api: Api<K> = Api::namespaced(client.clone(), namespace);
    let ret = api
        .create(&post_params, &resource)
        .await
        .map_err(|e| CommandError::new(format!("Unable to create {}", obj_name), Some(e.to_string()), None))?;
    info!("Creation of k8s {} returned {:?}", obj_name, ret);

    Ok(())
}

pub async fn kube_rollout_restart_statefulset(
    client: &kube::Client,
    namespace: &str,
    statefulset_name: &str,
) -> Result<(), CommandError> {
    info!("Restarting k8s StatefulSet {} in {}", statefulset_name, namespace);

    let api: Api<StatefulSet> = Api::namespaced(client.clone(), namespace);
    let ret = api.restart(statefulset_name).await.map_err(|e| {
        CommandError::new(
            format!("Unable to restart StatefulSet {}", statefulset_name),
            Some(e.to_string()),
            None,
        )
    })?;
    info!("Restart of k8s {} returned {:?}", statefulset_name, ret);

    Ok(())
}

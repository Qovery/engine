use kube::api::{DeleteParams, ListParams};
use kube::{Api, Resource};
use serde::de::DeserializeOwned;
use std::fmt::Debug;

pub async fn kube_delete_all_from_selector<K>(
    client: &kube::Client,
    selector: &str,
    namespace: &str,
) -> Result<(), kube::Error>
where
    K: Clone + DeserializeOwned + Debug + Resource,
    <K as Resource>::DynamicType: Default,
{
    let obj_name = K::kind(&K::DynamicType::default()).to_string();
    info!("Deleting k8s {} from selector {}", obj_name, selector);

    let list_params = ListParams::default().labels(selector);
    let delete_params = DeleteParams::background();

    let api: Api<K> = Api::namespaced(client.clone(), namespace);
    let ret = api.delete_collection(&delete_params, &list_params).await?;

    info!("Deletion of k8s {} matching {} returned {:?}", obj_name, selector, ret);

    Ok(())
}

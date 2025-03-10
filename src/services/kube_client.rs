use json_patch::PatchOperation;
use k8s_openapi::api::admissionregistration::v1::MutatingWebhookConfiguration;
use k8s_openapi::api::autoscaling::v1::Scale;
use k8s_openapi::api::core::v1::{Node, Service};
use k8s_openapi::api::{
    apps::v1::{Deployment, StatefulSet},
    core::v1::{Pod, Secret},
};
use kube::{
    Api, CustomResource,
    api::{ListParams, Patch, PatchParams},
    core::{ListMeta, ObjectList},
};
use serde_derive::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::ops::Deref;
use std::path::{Path, PathBuf};

use crate::environment::models::kubernetes::{K8sCrd, K8sDeployment, K8sMutatingWebhookConfiguration};
use crate::environment::models::kubernetes::{K8sPod, K8sSecret, K8sService, K8sStatefulset};
use crate::{
    errors::{CommandError, EngineError},
    events::EventDetails,
    runtime::block_on,
};
use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::config::{InClusterError, KubeConfigOptions, Kubeconfig, KubeconfigError};
use schemars::JsonSchema;

#[derive(Clone)]
pub struct QubeClient {
    client: kube::Client,
}

#[derive(Clone)]
pub enum SelectK8sResourceBy {
    All,
    // do not filter, select all resources
    Name(String),
    // select a named resource
    LabelsSelector(String), // select resources by labels
}

#[derive(CustomResource, Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[kube(group = "karpenter.k8s.aws", version = "v1", kind = "EC2NodeClass")]
pub struct Ec2nodeclassesSpec {}

impl Deref for QubeClient {
    type Target = kube::Client;

    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

impl AsRef<kube::Client> for QubeClient {
    fn as_ref(&self) -> &kube::Client {
        &self.client
    }
}

impl QubeClient {
    pub fn new(
        event_details: EventDetails,
        kubeconfig_path: Option<PathBuf>,
        kube_credentials: Vec<(String, String)>,
    ) -> Result<QubeClient, Box<EngineError>> {
        let kube_client = if let Some(kubeconfig_path) = &kubeconfig_path {
            block_on(create_kube_client(kubeconfig_path, kube_credentials.as_slice()))
                .map_err(|err| EngineError::new_cannot_connect_to_k8s_cluster(event_details.clone(), err))?
        } else {
            block_on(create_kube_client_in_cluster())
                .map_err(|err| EngineError::new_cannot_connect_to_k8s_cluster(event_details.clone(), err))?
        };

        Ok(QubeClient { client: kube_client })
    }

    pub async fn get_secrets(
        &self,
        event_details: EventDetails,
        namespace: Option<&str>,
        select_resource: SelectK8sResourceBy,
    ) -> Result<Vec<K8sSecret>, Box<EngineError>> {
        let client: Api<Secret> = match namespace {
            Some(namespace_name) => Api::namespaced(self.client.clone(), namespace_name),
            None => Api::all(self.client.clone()),
        };

        let mut labels = "".to_string();
        let params = match select_resource.clone() {
            SelectK8sResourceBy::LabelsSelector(x) => {
                labels = x;
                ListParams::default().labels(labels.as_str())
            }
            _ => ListParams::default(),
        };

        match select_resource {
            SelectK8sResourceBy::LabelsSelector(_) | SelectK8sResourceBy::All => match client.list(&params).await {
                Ok(x) => Ok(K8sSecret::from_k8s_secret_objectlist(event_details, x)),
                Err(e) if Self::is_error_code(&e, 404) => Ok(vec![]),
                Err(e) => Err(Box::new(EngineError::new_k8s_get_deployment_error(
                    event_details,
                    CommandError::new_from_safe_message(format!(
                        "Error while trying to get kubernetes pods with labels `{labels}`. {e}"
                    )),
                ))),
            },
            SelectK8sResourceBy::Name(pod_name) => match client.get(pod_name.as_str()).await {
                Ok(x) => Ok(vec![K8sSecret::from_k8s_secret(event_details, x)?]),
                Err(e) if Self::is_error_code(&e, 404) => Ok(vec![]),
                Err(e) => Err(Box::new(EngineError::new_k8s_get_deployment_error(
                    event_details,
                    CommandError::new_from_safe_message(format!(
                        "Error while trying to get kubernetes pods from {pod_name}/{}. {e}",
                        namespace.unwrap_or("no namespace")
                    )),
                ))),
            },
        }
    }

    /// Patch an existing secret
    ///
    /// Patch should looks like the content of the secret data, but in json:
    /// ```
    /// let patch = serde_json::json!({
    ///     "data": {
    ///         "release": "encoded_base64_string",
    ///     }
    /// });
    /// ```
    pub async fn patch_secret(
        &self,
        event_details: EventDetails,
        name: &str,
        namespace: &str,
        patch: serde_json::Value,
    ) -> Result<(), Box<EngineError>> {
        let client: Api<Secret> = Api::namespaced(self.client.clone(), namespace);

        let patch_params = PatchParams {
            field_manager: Some(name.to_string()),
            ..Default::default()
        };

        match client.patch(name, &patch_params, &Patch::Merge(&patch)).await {
            Ok(_) => Ok(()),
            Err(e) => Err(Box::new(EngineError::new_k8s_patch_secret_error(
                event_details,
                CommandError::new_from_safe_message(format!("Error while trying to patch kubernetes secret. {e}")),
            ))),
        }
    }

    pub async fn get_nodes(
        &self,
        event_details: EventDetails,
        select_resource: SelectK8sResourceBy,
    ) -> Result<Vec<Node>, Box<EngineError>> {
        let client: Api<Node> = Api::all(self.client.clone());

        let mut labels = "".to_string();
        let params = match select_resource.clone() {
            SelectK8sResourceBy::LabelsSelector(x) => {
                labels = x;
                ListParams::default().labels(labels.as_str())
            }
            _ => ListParams::default(),
        };

        match client.list(&params).await {
            Ok(node_list) => Ok(node_list.items),
            Err(e) if Self::is_error_code(&e, 404) => Ok(vec![]),
            Err(e) => Err(Box::new(EngineError::new_k8s_cannot_get_nodes(
                event_details,
                CommandError::new_from_safe_message(format!(
                    "Error while trying to get kubernetes nodes with labels `{labels}`. {e}"
                )),
            ))),
        }
    }

    pub async fn patch_node(
        &self,
        event_details: EventDetails,
        node: Node,
        patch_operations: &[PatchOperation],
    ) -> Result<(), Box<EngineError>> {
        let client: Api<Node> = Api::all(self.client.clone());

        let json_patch = json_patch::Patch(patch_operations.to_vec());
        let patch: Patch<Scale> = Patch::Json(json_patch);

        if let Some(name) = node.metadata.name {
            if let Err(e) = client.patch(&name, &PatchParams::default(), &patch).await {
                return Err(Box::new(EngineError::new_k8s_patch_node_error(
                    event_details,
                    CommandError::new_from_safe_message(format!("Error while trying to patch a kubernetes node. {e}")),
                )));
            }
        }

        Ok(())
    }

    pub async fn get_pods(
        &self,
        event_details: EventDetails,
        namespace: Option<&str>,
        select_resource: SelectK8sResourceBy,
    ) -> Result<Vec<K8sPod>, Box<EngineError>> {
        let client: Api<Pod> = match namespace {
            Some(namespace_name) => Api::namespaced(self.client.clone(), namespace_name),
            None => Api::all(self.client.clone()),
        };

        let mut labels = "".to_string();
        let params = match select_resource.clone() {
            SelectK8sResourceBy::LabelsSelector(x) => {
                labels = x;
                ListParams::default().labels(labels.as_str())
            }
            _ => ListParams::default(),
        };

        match select_resource {
            SelectK8sResourceBy::LabelsSelector(_) | SelectK8sResourceBy::All => match client.list(&params).await {
                Ok(x) => Ok(K8sPod::from_k8s_pod_objectlist(event_details, x)),
                Err(e) if Self::is_error_code(&e, 404) => Ok(vec![]),
                Err(e) => Err(Box::new(EngineError::new_k8s_get_deployment_error(
                    event_details,
                    CommandError::new_from_safe_message(format!(
                        "Error while trying to get kubernetes pods with labels `{labels}`. {e}"
                    )),
                ))),
            },
            SelectK8sResourceBy::Name(pod_name) => match client.get(pod_name.as_str()).await {
                Ok(x) => Ok(vec![K8sPod::from_k8s_pod(event_details, x)?]),
                Err(e) if Self::is_error_code(&e, 404) => Ok(vec![]),
                Err(e) => Err(Box::new(EngineError::new_k8s_get_deployment_error(
                    event_details,
                    CommandError::new_from_safe_message(format!(
                        "Error while trying to get kubernetes pods from {pod_name}/{}. {e}",
                        namespace.unwrap_or("no namespace")
                    )),
                ))),
            },
        }
    }

    pub async fn get_deployments_from_api(
        &self,
        event_details: EventDetails,
        namespace: Option<&str>,
        select_resource: SelectK8sResourceBy,
    ) -> Result<Option<ObjectList<Deployment>>, Box<EngineError>> {
        let client: Api<Deployment> = match namespace {
            Some(namespace_name) => Api::namespaced(self.client.clone(), namespace_name),
            None => Api::all(self.client.clone()),
        };

        let mut labels = "".to_string();
        let params = match select_resource.clone() {
            SelectK8sResourceBy::LabelsSelector(x) => {
                labels = x;
                ListParams::default().labels(labels.as_str())
            }
            _ => ListParams::default(),
        };

        match select_resource {
            SelectK8sResourceBy::LabelsSelector(_) | SelectK8sResourceBy::All => match client.list(&params).await {
                Ok(x) => Ok(Some(x)),
                Err(e) if Self::is_error_code(&e, 404) => Ok(None),
                Err(e) => Err(Box::new(EngineError::new_k8s_get_deployment_error(
                    event_details,
                    CommandError::new_from_safe_message(format!(
                        "Error while trying to get kubernetes deployments with labels `{labels}`. {e}"
                    )),
                ))),
            },
            SelectK8sResourceBy::Name(deployment_name) => match client.get(deployment_name.as_str()).await {
                Ok(x) => Ok(Some(ObjectList::<Deployment> {
                    types: Default::default(),
                    metadata: ListMeta::default(),
                    items: vec![x],
                })),
                Err(e) if Self::is_error_code(&e, 404) => Ok(None),
                Err(e) => Err(Box::new(EngineError::new_k8s_get_deployment_error(
                    event_details,
                    CommandError::new_from_safe_message(format!(
                        "Error while trying to get kubernetes deployments from {deployment_name}/{}. {e}",
                        namespace.unwrap_or("no namespace")
                    )),
                ))),
            },
        }
    }

    async fn get_mutating_webhook_configurations_from_api(
        &self,
        event_details: EventDetails,
        select_resource: SelectK8sResourceBy,
    ) -> Result<Option<ObjectList<MutatingWebhookConfiguration>>, Box<EngineError>> {
        let client: Api<MutatingWebhookConfiguration> = Api::all(self.client.clone());

        let mut labels = "".to_string();
        let params = match select_resource.clone() {
            SelectK8sResourceBy::LabelsSelector(x) => {
                labels = x;
                ListParams::default().labels(labels.as_str())
            }
            _ => ListParams::default(),
        };

        match select_resource {
            SelectK8sResourceBy::LabelsSelector(_) | SelectK8sResourceBy::All => match client.list(&params).await {
                Ok(x) => Ok(Some(x)),
                Err(e) if Self::is_error_code(&e, 404) => Ok(None),
                Err(e) => Err(Box::new(EngineError::new_k8s_get_mutating_webhook_configuration_error(
                    event_details,
                    CommandError::new_from_safe_message(format!(
                        "Error while trying to get kubernetes mutating webhook configuration with labels `{labels}`. {e}"
                    )),
                ))),
            },
            SelectK8sResourceBy::Name(webhook) => match client.get(webhook.as_str()).await {
                Ok(x) => Ok(Some(ObjectList::<MutatingWebhookConfiguration> {
                    types: Default::default(),
                    metadata: ListMeta::default(),
                    items: vec![x],
                })),
                Err(e) if Self::is_error_code(&e, 404) => Ok(None),
                Err(e) => Err(Box::new(EngineError::new_k8s_get_mutating_webhook_configuration_error(
                    event_details,
                    CommandError::new_from_safe_message(format!(
                        "Error while trying to get kubernetes mutating webhook configuration from {webhook}. {e}",
                    )),
                ))),
            },
        }
    }

    pub async fn get_mutating_webhook_configurations(
        &self,
        event_details: EventDetails,
        select_resource: SelectK8sResourceBy,
    ) -> Result<Vec<K8sMutatingWebhookConfiguration>, Box<EngineError>> {
        match Self::get_mutating_webhook_configurations_from_api(self, event_details.clone(), select_resource).await? {
            Some(x) => Ok(
                K8sMutatingWebhookConfiguration::from_k8s_mutating_webhook_configuration_objectlist(event_details, x),
            ),
            None => Ok(vec![]),
        }
    }

    pub async fn get_deployments(
        &self,
        event_details: EventDetails,
        namespace: Option<&str>,
        select_resource: SelectK8sResourceBy,
    ) -> Result<Vec<K8sDeployment>, Box<EngineError>> {
        match Self::get_deployments_from_api(self, event_details.clone(), namespace, select_resource).await? {
            Some(x) => Ok(K8sDeployment::from_k8s_deployment_objectlist(event_details, x)),
            None => Ok(vec![]),
        }
    }

    pub async fn set_deployment_replicas_number(
        &self,
        event_details: EventDetails,
        name: &str,
        namespace: &str,
        replicas: u32,
    ) -> Result<(), Box<EngineError>> {
        let client: Api<Deployment> = Api::namespaced(self.client.clone(), namespace);

        let patch = json!({
            "spec": {
                "replicas": replicas
            }
        });
        let patch_params = PatchParams {
            field_manager: Some(name.to_string()),
            ..Default::default()
        };

        match client.patch(name, &patch_params, &Patch::Merge(&patch)).await {
            Ok(_) => Ok(()),
            Err(e) => Err(Box::new(EngineError::new_k8s_scale_replicas(
                event_details,
                name.to_string(),
                namespace.to_string(),
                replicas,
                CommandError::new_from_safe_message(format!(
                    "Error while trying to set kubernetes deployment replicas. {e}"
                )),
            ))),
        }
    }

    pub async fn delete_deployment_from_name(
        &self,
        event_details: EventDetails,
        namespace: &str,
        name: &str,
    ) -> Result<(), Box<EngineError>> {
        let client: Api<Deployment> = Api::namespaced(self.client.clone(), namespace);

        match client.delete(name, &Default::default()).await {
            Ok(_) => Ok(()),
            Err(e) => Err(Box::new(EngineError::new_k8s_delete_deployment_error(
                event_details,
                CommandError::new_from_safe_message(format!("Error while trying to delete kubernetes deployment. {e}")),
            ))),
        }
    }

    pub async fn delete_service_from_name(
        &self,
        event_details: EventDetails,
        namespace: &str,
        name: &str,
    ) -> Result<(), Box<EngineError>> {
        let client: Api<Service> = Api::namespaced(self.client.clone(), namespace);

        match client.delete(name, &Default::default()).await {
            Ok(_) => Ok(()),
            Err(e) => Err(Box::new(EngineError::new_k8s_delete_service_error(
                event_details,
                CommandError::new_from_safe_message(format!("Error while trying to delete kubernetes service. {e}")),
                "Error while trying to delete kubernetes service".to_string(),
            ))),
        }
    }

    pub async fn get_services(
        &self,
        event_details: EventDetails,
        namespace: Option<&str>,
        select_resource: SelectK8sResourceBy,
    ) -> Result<Vec<K8sService>, Box<EngineError>> {
        let client: Api<Service> = match namespace {
            Some(namespace_name) => Api::namespaced(self.client.clone(), namespace_name),
            None => Api::all(self.client.clone()),
        };

        let mut labels = "".to_string();
        let params = match select_resource.clone() {
            SelectK8sResourceBy::LabelsSelector(x) => {
                labels = x;
                ListParams::default().labels(labels.as_str())
            }
            _ => ListParams::default(),
        };

        match select_resource {
            SelectK8sResourceBy::LabelsSelector(_) | SelectK8sResourceBy::All => match client.list(&params).await {
                Ok(x) => Ok(K8sService::from_k8s_service_objectlist(event_details, x)),
                Err(e) if Self::is_error_code(&e, 404) => Ok(vec![]),
                Err(e) => Err(Box::new(EngineError::new_k8s_cannot_get_services(
                    event_details,
                    CommandError::new_from_safe_message(format!(
                        "Error while trying to get kubernetes service with labels `{labels}`. {e}"
                    )),
                ))),
            },
            SelectK8sResourceBy::Name(service_name) => match client.get(service_name.as_str()).await {
                Ok(x) => Ok(vec![K8sService::from_k8s_service(event_details, x)?]),
                Err(e) if Self::is_error_code(&e, 404) => Ok(vec![]),
                Err(e) => Err(Box::new(EngineError::new_k8s_cannot_get_services(
                    event_details,
                    CommandError::new_from_safe_message(format!(
                        "Error while trying to get kubernetes service from {service_name}/{}. {e}",
                        namespace.unwrap_or("no namespace")
                    )),
                ))),
            },
        }
    }

    pub async fn get_statefulsets(
        &self,
        event_details: EventDetails,
        namespace: Option<&str>,
        select_resource: SelectK8sResourceBy,
    ) -> Result<Vec<K8sStatefulset>, Box<EngineError>> {
        let client: Api<StatefulSet> = match namespace {
            Some(namespace_name) => Api::namespaced(self.client.clone(), namespace_name),
            None => Api::all(self.client.clone()),
        };

        let mut labels = "".to_string();
        let params = match select_resource.clone() {
            SelectK8sResourceBy::LabelsSelector(x) => {
                labels = x;
                ListParams::default().labels(labels.as_str())
            }
            _ => ListParams::default(),
        };

        match select_resource {
            SelectK8sResourceBy::LabelsSelector(_) | SelectK8sResourceBy::All => match client.list(&params).await {
                Ok(x) => Ok(K8sStatefulset::from_k8s_statefulset_objectlist(event_details, x)),
                Err(e) if Self::is_error_code(&e, 404) => Ok(vec![]),
                Err(e) => Err(Box::new(EngineError::new_k8s_get_statefulset_error(
                    event_details,
                    CommandError::new_from_safe_message(format!(
                        "Error while trying to get kubernetes statefulset with labels `{labels}`. {e}"
                    )),
                ))),
            },
            SelectK8sResourceBy::Name(statfulset_name) => match client.get(statfulset_name.as_str()).await {
                Ok(x) => Ok(vec![K8sStatefulset::from_k8s_statefulset(event_details, x)?]),
                Err(e) if Self::is_error_code(&e, 404) => Ok(vec![]),
                Err(e) => Err(Box::new(EngineError::new_k8s_get_statefulset_error(
                    event_details,
                    CommandError::new_from_safe_message(format!(
                        "Error while trying to get kubernetes statefulset from {statfulset_name}/{}. {e}",
                        namespace.unwrap_or("no namespace")
                    )),
                ))),
            },
        }
    }

    pub async fn set_statefulset_replicas_number(
        &self,
        event_details: EventDetails,
        name: &str,
        namespace: &str,
        replicas: u32,
    ) -> Result<(), Box<EngineError>> {
        let client: Api<StatefulSet> = Api::namespaced(self.client.clone(), namespace);

        let patch = json!({
            "spec": {
                "replicas": replicas
            }
        });
        let patch_params = PatchParams {
            field_manager: Some(name.to_string()),
            ..Default::default()
        };

        match client.patch(name, &patch_params, &Patch::Merge(patch)).await {
            Ok(_) => Ok(()),
            Err(e) => Err(Box::new(EngineError::new_k8s_scale_replicas(
                event_details,
                name.to_string(),
                namespace.to_string(),
                replicas,
                CommandError::new_from_safe_message(format!(
                    "Error while trying to set kubernetes statefulset replicas. {e}"
                )),
            ))),
        }
    }

    pub async fn get_crds(
        &self,
        event_details: EventDetails,
        select_resource: SelectK8sResourceBy,
    ) -> Result<Vec<K8sCrd>, Box<EngineError>> {
        let client: Api<CustomResourceDefinition> = Api::all(self.client.clone());

        let mut labels = "".to_string();
        let params = match select_resource.clone() {
            SelectK8sResourceBy::LabelsSelector(x) => {
                labels = x;
                ListParams::default().labels(labels.as_str())
            }
            _ => ListParams::default(),
        };

        match select_resource {
            SelectK8sResourceBy::LabelsSelector(_) | SelectK8sResourceBy::All => match client.list(&params).await {
                Ok(x) => Ok(K8sCrd::from_k8s_crd_objectlist(event_details, x)),
                Err(e) if Self::is_error_code(&e, 404) => Ok(vec![]),
                Err(e) => Err(Box::new(EngineError::new_k8s_get_crd_error(
                    event_details,
                    CommandError::new_from_safe_message(format!(
                        "Error while trying to get kubernetes crds with labels `{labels}`. {e}"
                    )),
                ))),
            },
            SelectK8sResourceBy::Name(crd_name) => match client.get(crd_name.as_str()).await {
                Ok(x) => Ok(vec![K8sCrd::from_k8s_crd(event_details, x)?]),
                Err(e) if Self::is_error_code(&e, 404) => Ok(vec![]),
                Err(e) => Err(Box::new(EngineError::new_k8s_get_crd_error(
                    event_details,
                    CommandError::new_from_safe_message(format!(
                        "Error while trying to get kubernetes crds from {crd_name}. {e}",
                    )),
                ))),
            },
        }
    }

    pub async fn get_ec2_node_classes(
        &self,
        event_details: &EventDetails,
    ) -> Result<Vec<EC2NodeClass>, Box<EngineError>> {
        let client: Api<EC2NodeClass> = Api::all(self.client.clone());
        let params = ListParams::default();

        match client.list(&params).await {
            Ok(x) => Ok(x.items),
            Err(e) => Err(Box::new(EngineError::new_k8s_get_deployment_error(
                event_details.clone(),
                CommandError::new_from_safe_message(format!("Error while trying to get Ec2NodeClasses {e}")),
            ))),
        }
    }

    fn is_error_code(e: &kube::Error, http_code_number: u16) -> bool {
        matches!(e, kube::Error::Api(x) if x.code == http_code_number)
    }

    pub fn client(&self) -> kube::Client {
        self.client.clone()
    }
}

async fn create_kube_client<P: AsRef<Path>>(
    kubeconfig_path: P,
    envs: &[(String, String)],
) -> Result<kube::Client, kube::Error> {
    let to_err = |err: KubeconfigError| -> kube::Error {
        kube::Error::Service(Box::<dyn std::error::Error + Send + Sync>::from(err.to_string()))
    };

    // Read kube config
    let mut kubeconfig = Kubeconfig::read_from(kubeconfig_path).map_err(to_err)?;

    // Inject our env variables if needed
    for auth in kubeconfig.auth_infos.iter_mut() {
        if let Some(exec_config) = &mut auth.auth_info.as_mut().and_then(|auth| auth.exec.as_mut()) {
            let exec_envs = exec_config.env.get_or_insert(vec![]);
            for (k, v) in envs {
                let mut hash_map = HashMap::with_capacity(2);
                hash_map.insert("name".to_string(), k.to_string());
                hash_map.insert("value".to_string(), v.to_string());
                exec_envs.push(hash_map);
            }
        }
    }

    // build kube client: the kube config must have already the good context selected
    let kube_config = kube::Config::from_custom_kubeconfig(kubeconfig, &KubeConfigOptions::default())
        .await
        .map_err(to_err)?;
    let kube_client = kube::Client::try_from(kube_config)?;

    // Try to contact the api to verify we are correctly connected
    kube_client.apiserver_version().await?;
    Ok(kube_client)
}

async fn create_kube_client_in_cluster() -> Result<kube::Client, kube::Error> {
    let to_err = |err: InClusterError| -> kube::Error {
        kube::Error::Service(Box::<dyn std::error::Error + Send + Sync>::from(err.to_string()))
    };

    // build kube client: the kube config must have already the good context selected
    let kube_config = kube::Config::incluster().map_err(to_err)?;
    let kube_client = kube::Client::try_from(kube_config)?;

    // Try to contact the api to verify we are correctly connected
    kube_client.apiserver_version().await?;
    Ok(kube_client)
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::path::PathBuf;

    use uuid::Uuid;

    use crate::runtime::block_on;
    use crate::services::kube_client::SelectK8sResourceBy;

    use crate::{
        events::{EventDetails, Stage},
        io_models::QoveryIdentifier,
    };

    use super::QubeClient;

    pub fn get_qube_client() -> (QubeClient, EventDetails) {
        let kubeconfig = env::var("HOME").unwrap() + "/.kube/config";
        let uuid = Uuid::new_v4();
        let qovery_id = QoveryIdentifier::new(uuid);
        let event_details = EventDetails::new(
            None,
            qovery_id.clone(),
            qovery_id,
            uuid.to_string(),
            Stage::Environment(crate::events::EnvironmentStep::ValidateSystemRequirements),
            crate::events::Transmitter::Application(uuid, "".to_string()),
        );
        let quke_client = QubeClient::new(event_details.clone(), Some(PathBuf::from(kubeconfig)), vec![]);
        assert!(quke_client.is_ok());
        (quke_client.unwrap(), event_details)
    }

    #[test]
    #[cfg(feature = "test-local-kube")]
    pub fn k8s_get_deployments() {
        // by default, there are deployments, so we should fine things
        use crate::runtime::block_on;
        let (qube_client, event_details) = get_qube_client();
        let all_deployments =
            block_on(qube_client.get_deployments(event_details.clone(), None, SelectK8sResourceBy::All));
        assert!(all_deployments.is_ok());
        assert!(!all_deployments.unwrap().is_empty());
        // coredns is by default available in k3d
        let coredns = block_on(qube_client.get_deployments(
            event_details.clone(),
            Some("kube-system"),
            SelectK8sResourceBy::Name("coredns".to_string()),
        ));
        assert!(coredns.is_ok());
        // find coredns in kube-system by selectors
        let coredns = block_on(qube_client.get_deployments(
            event_details,
            Some("kube-system"),
            SelectK8sResourceBy::LabelsSelector("k8s-app=kube-dns".to_string()),
        ));
        assert!(coredns.is_ok());
        assert!(!coredns.unwrap().is_empty());
    }

    #[test]
    #[cfg(feature = "test-local-kube")]
    pub fn set_deployment_replicas() {
        use crate::runtime::block_on;
        let (qube_client, event_details) = get_qube_client();
        // get coredns deployed by default on k3d
        let coredns = block_on(qube_client.get_deployments(
            event_details.clone(),
            Some("kube-system"),
            SelectK8sResourceBy::Name("coredns".to_string()),
        ));
        assert!(coredns.is_ok());
        let coredns_list = coredns.unwrap();
        assert!(!coredns_list.is_empty());
        let coredns = coredns_list.first().unwrap();
        // scale replicas to 2
        let set_replicas = block_on(qube_client.set_deployment_replicas_number(
            event_details,
            coredns.metadata.name.as_str(),
            coredns.metadata.namespace.as_str(),
            2,
        ));
        assert!(set_replicas.is_ok());
    }

    #[test]
    #[cfg(feature = "test-local-kube")]
    pub fn k8s_get_secrets() {
        let (qube_client, event_details) = get_qube_client();
        let all_secrets = block_on(qube_client.get_secrets(event_details, None, SelectK8sResourceBy::All)).unwrap();
        // there are secrets by default on a fresh K8s cluster, so it shouldn't be empty
        assert!(!all_secrets.is_empty());
    }

    #[test]
    #[cfg(feature = "test-local-kube")]
    pub fn k8s_error_management_code() {
        let code_ok = QubeClient::is_error_code(
            &kube::Error::Api(kube::error::ErrorResponse {
                code: 404,
                message: "".to_string(),
                reason: "".to_string(),
                status: "".to_string(),
            }),
            404,
        );
        assert!(code_ok);

        let code_error = QubeClient::is_error_code(
            &kube::Error::Api(kube::error::ErrorResponse {
                code: 200,
                message: "".to_string(),
                reason: "".to_string(),
                status: "".to_string(),
            }),
            404,
        );
        assert!(!code_error);
    }
}

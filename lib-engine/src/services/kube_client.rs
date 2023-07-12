use k8s_openapi::api::{
    apps::v1::{Deployment, StatefulSet},
    core::v1::Pod,
};
use kube::{
    api::{ListParams, Patch, PatchParams},
    Api,
};
use serde_json::json;

use crate::{
    errors::{CommandError, EngineError},
    events::EventDetails,
    models::kubernetes::{K8sDeployment, K8sPod, K8sStatefulset},
    runtime::block_on,
    utilities::create_kube_client,
};

#[derive(Clone)]
pub struct QubeClient {
    client: kube::Client,
}

#[derive(Clone)]
pub enum SelectK8sResourceBy {
    All,                    // do not filter, select all resources
    Name(String),           // select a named resource
    LabelsSelector(String), // select resources by labels
}

impl QubeClient {
    pub fn new(
        event_details: EventDetails,
        kubeconfig_path: String,
        kube_credentials: Vec<(String, String)>,
    ) -> Result<QubeClient, Box<EngineError>> {
        let kube_client = block_on(create_kube_client(kubeconfig_path, kube_credentials.as_slice()))
            .map_err(|err| Box::new(EngineError::new_cannot_connect_to_k8s_cluster(event_details, err)))?;
        Ok(QubeClient { client: kube_client })
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
                Err(e) => Err(Box::new(EngineError::new_k8s_get_deployment_error(
                    event_details,
                    CommandError::new_from_safe_message(format!(
                        "Error while trying to get kubernetes pods with labels `{labels}`. {e}"
                    )),
                ))),
            },
            SelectK8sResourceBy::Name(pod_name) => match client.get(pod_name.as_str()).await {
                Ok(x) => Ok(vec![K8sPod::from_k8s_pod(event_details, x)?]),
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

    pub async fn get_deployments(
        &self,
        event_details: EventDetails,
        namespace: Option<&str>,
        select_resource: SelectK8sResourceBy,
    ) -> Result<Vec<K8sDeployment>, Box<EngineError>> {
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
                Ok(x) => Ok(K8sDeployment::from_k8s_deployment_objectlist(event_details, x)),
                Err(e) => Err(Box::new(EngineError::new_k8s_get_deployment_error(
                    event_details,
                    CommandError::new_from_safe_message(format!(
                        "Error while trying to get kubernetes deployments with labels `{labels}`. {e}"
                    )),
                ))),
            },
            SelectK8sResourceBy::Name(deployment_name) => match client.get(deployment_name.as_str()).await {
                Ok(x) => Ok(vec![K8sDeployment::from_k8s_deployment(event_details, x)?]),
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
                Err(e) => Err(Box::new(EngineError::new_k8s_get_statefulset_error(
                    event_details,
                    CommandError::new_from_safe_message(format!(
                        "Error while trying to get kubernetes statefulset with labels `{labels}`. {e}"
                    )),
                ))),
            },
            SelectK8sResourceBy::Name(statfulset_name) => match client.get(statfulset_name.as_str()).await {
                Ok(x) => Ok(vec![K8sStatefulset::from_k8s_statefulset(event_details, x)?]),
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
}
#[cfg(test)]
mod tests {
    use std::env;

    use uuid::Uuid;

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
        let quke_client = QubeClient::new(event_details.clone(), kubeconfig, vec![]);
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
            block_on(qube_client.get_deployments(event_details.clone(), None, super::SelectK8sResourceBy::All));
        assert!(all_deployments.is_ok());
        assert!(!all_deployments.unwrap().is_empty());
        // coredns is by default available in k3d
        let coredns = block_on(qube_client.get_deployments(
            event_details.clone(),
            Some("kube-system"),
            super::SelectK8sResourceBy::Name("coredns".to_string()),
        ));
        assert!(coredns.is_ok());
        // find coredns in kube-system by selectors
        let coredns = block_on(qube_client.get_deployments(
            event_details,
            Some("kube-system"),
            super::SelectK8sResourceBy::LabelsSelector("k8s-app=kube-dns".to_string()),
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
            super::SelectK8sResourceBy::Name("coredns".to_string()),
        ));
        assert!(coredns.is_ok());
        let coredns_list = coredns.unwrap();
        assert!(!coredns_list.is_empty());
        let coredns = coredns_list.get(0).unwrap();
        // scale replicas to 2
        let set_replicas = block_on(qube_client.set_deployment_replicas_number(
            event_details,
            coredns.metadata.name.as_str(),
            coredns.metadata.namespace.as_str(),
            2,
        ));
        assert!(set_replicas.is_ok());
    }
}

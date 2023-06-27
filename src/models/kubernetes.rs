use k8s_openapi::api::apps::v1::{Deployment, DeploymentStatus, StatefulSet, StatefulSetStatus};
use kube::core::ObjectList;

use crate::{
    errors::{CommandError, EngineError},
    events::EventDetails,
};

pub struct K8sDeployment {
    pub metadata: K8sMetadata,
    pub status: Option<K8sDeploymentStatus>,
}

pub struct K8sStatefulset {
    pub metadata: K8sMetadata,
    pub status: Option<K8sStatefulsetStatus>,
}

pub struct K8sMetadata {
    pub name: String,
    pub namespace: String,
}

pub struct K8sDeploymentStatus {
    pub replicas: Option<i32>,
    pub ready_replicas: Option<i32>,
}

pub struct K8sStatefulsetStatus {
    pub replicas: i32,
    pub ready_replicas: Option<i32>,
}

impl K8sDeploymentStatus {
    pub fn from_k8s_deployment_status(k8s_deployment_status: DeploymentStatus) -> K8sDeploymentStatus {
        K8sDeploymentStatus {
            replicas: k8s_deployment_status.replicas,
            ready_replicas: k8s_deployment_status.ready_replicas,
        }
    }
}

impl K8sStatefulsetStatus {
    pub fn from_k8s_statefulset_status(k8s_statefulset_status: StatefulSetStatus) -> K8sStatefulsetStatus {
        K8sStatefulsetStatus {
            replicas: k8s_statefulset_status.replicas,
            ready_replicas: k8s_statefulset_status.ready_replicas,
        }
    }
}

impl K8sDeployment {
    pub fn from_k8s_deployment_objectlist(
        event_details: EventDetails,
        k8s_deployments: ObjectList<Deployment>,
    ) -> Vec<K8sDeployment> {
        let mut deployments: Vec<K8sDeployment> = Vec::with_capacity(k8s_deployments.items.len());

        for deploy in k8s_deployments.items {
            if let Ok(x) = K8sDeployment::from_k8s_deployment(event_details.clone(), deploy) {
                deployments.push(x);
            };
        }
        deployments
    }

    pub fn from_k8s_deployment(
        event_details: EventDetails,
        k8s_deployment: Deployment,
    ) -> Result<K8sDeployment, Box<EngineError>> {
        let deployment_status = k8s_deployment
            .status
            .map(K8sDeploymentStatus::from_k8s_deployment_status);

        Ok(K8sDeployment {
            metadata: K8sMetadata {
                name: match k8s_deployment.metadata.name.clone() {
                    Some(x) => x,
                    None => {
                        return Err(Box::new(EngineError::new_k8s_get_deployment_error(
                            event_details,
                            CommandError::new_from_safe_message(
                                "can't read kubernetes deployment, name is missing".to_string(),
                            ),
                        )))
                    }
                },
                namespace: match k8s_deployment.metadata.namespace {
                    Some(x) => x,
                    None => {
                        return Err(Box::new(EngineError::new_k8s_get_deployment_error(
                            event_details,
                            CommandError::new_from_safe_message(format!(
                                "can't read kubernetes deployment, namespace is missing for deployment name `{}`",
                                k8s_deployment.metadata.name.unwrap_or("unknown".to_string())
                            )),
                        )))
                    }
                },
            },
            status: deployment_status,
        })
    }
}

impl K8sStatefulset {
    pub fn from_k8s_statefulset_objectlist(
        event_details: EventDetails,
        k8s_statefulsets: ObjectList<StatefulSet>,
    ) -> Vec<K8sStatefulset> {
        let mut statefulsets: Vec<K8sStatefulset> = Vec::with_capacity(k8s_statefulsets.items.len());

        for statefulset in k8s_statefulsets.items {
            if let Ok(x) = K8sStatefulset::from_k8s_statefulset(event_details.clone(), statefulset) {
                statefulsets.push(x);
            };
        }
        statefulsets
    }

    pub fn from_k8s_statefulset(
        event_details: EventDetails,
        k8s_statefulset: StatefulSet,
    ) -> Result<K8sStatefulset, Box<EngineError>> {
        let statefulset_status = k8s_statefulset
            .status
            .map(K8sStatefulsetStatus::from_k8s_statefulset_status);

        Ok(K8sStatefulset {
            metadata: K8sMetadata {
                name: match k8s_statefulset.metadata.name.clone() {
                    Some(x) => x,
                    None => {
                        return Err(Box::new(EngineError::new_k8s_get_statefulset_error(
                            event_details,
                            CommandError::new_from_safe_message(
                                "can't read kubernetes statefulset, name is missing".to_string(),
                            ),
                        )))
                    }
                },
                namespace: match k8s_statefulset.metadata.namespace {
                    Some(x) => x,
                    None => {
                        return Err(Box::new(EngineError::new_k8s_get_statefulset_error(
                            event_details,
                            CommandError::new_from_safe_message(format!(
                                "can't read kubernetes statefulset, namespace is missing for deployment name `{}`",
                                k8s_statefulset.metadata.name.unwrap_or("unknown".to_string())
                            )),
                        )))
                    }
                },
            },
            status: statefulset_status,
        })
    }
}

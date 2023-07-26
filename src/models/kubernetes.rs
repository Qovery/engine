use std::{collections::BTreeMap, io::Read};

use chrono::Duration;
use k8s_openapi::api::{
    apps::v1::{Deployment, DeploymentStatus, StatefulSet, StatefulSetStatus},
    core::v1::{Pod, PodStatus, Secret},
};
use k8s_openapi::ByteString;
use kube::core::ObjectList;

use crate::{
    cloud_provider::helm::ChartReleaseData,
    errors::{CommandError, EngineError},
    events::EventDetails,
};

/**********
PODS & COMMON
***********/

pub struct K8sPod {
    pub metadata: K8sMetadata,
    pub status: K8sPodStatus,
}

pub struct K8sPodStatus {
    pub phase: K8sPodPhase,
}

#[derive(Default, Debug)]
pub enum K8sPodPhase {
    Pending,
    Running,
    Succeeded,
    Failed,
    #[default]
    Unknown,
}

#[derive(Debug)]
pub struct K8sMetadata {
    pub name: String,
    pub namespace: String,
    //#[serde(rename(deserialize = "deletion_grace_period_seconds"))]
    pub termination_grace_period_seconds: Option<Duration>,
}

pub struct K8sStatefulsetStatus {
    pub replicas: i32,
    pub ready_replicas: Option<i32>,
}

impl K8sPodStatus {
    pub fn from_k8s_pod_status(k8s_pod_status: Option<PodStatus>) -> K8sPodStatus {
        let phase = match k8s_pod_status {
            Some(x) => x.phase,
            None => None,
        };
        K8sPodStatus {
            phase: K8sPodPhase::from_k8s_pod_phase(phase),
        }
    }
}

impl K8sPodPhase {
    pub fn from_k8s_pod_phase(phase: Option<String>) -> K8sPodPhase {
        match phase {
            Some(x) => match x.as_str() {
                "Pending" => K8sPodPhase::Pending,
                "Running" => K8sPodPhase::Running,
                "Succeeded" => K8sPodPhase::Succeeded,
                "Failed" => K8sPodPhase::Failed,
                _ => K8sPodPhase::Unknown,
            },
            None => K8sPodPhase::Unknown,
        }
    }
}

impl K8sPod {
    pub fn from_k8s_pod_objectlist(event_details: EventDetails, k8s_pods: ObjectList<Pod>) -> Vec<K8sPod> {
        let mut pods: Vec<K8sPod> = Vec::with_capacity(k8s_pods.items.len());

        for deploy in k8s_pods.items {
            if let Ok(x) = K8sPod::from_k8s_pod(event_details.clone(), deploy) {
                pods.push(x);
            };
        }
        pods
    }

    pub fn from_k8s_pod(event_details: EventDetails, k8s_pod: Pod) -> Result<K8sPod, Box<EngineError>> {
        let pod_status = K8sPodStatus::from_k8s_pod_status(k8s_pod.status);

        Ok(K8sPod {
            metadata: K8sMetadata {
                name: match k8s_pod.metadata.name.clone() {
                    Some(x) => x,
                    None => {
                        return Err(Box::new(EngineError::new_k8s_get_pod_error(
                            event_details,
                            CommandError::new_from_safe_message(
                                "can't read kubernetes pod, name is missing".to_string(),
                            ),
                        )))
                    }
                },
                namespace: match k8s_pod.metadata.namespace {
                    Some(x) => x,
                    None => {
                        return Err(Box::new(EngineError::new_k8s_get_pod_error(
                            event_details,
                            CommandError::new_from_safe_message(format!(
                                "can't read kubernetes pod, namespace is missing for pod name `{}`",
                                k8s_pod.metadata.name.unwrap_or("unknown".to_string())
                            )),
                        )))
                    }
                },
                termination_grace_period_seconds: k8s_pod.metadata.deletion_grace_period_seconds.map(Duration::seconds),
            },
            status: pod_status,
        })
    }
}

/**********
DEPLOYMENTS
***********/

pub struct K8sDeployment {
    pub metadata: K8sMetadata,
    pub status: Option<K8sDeploymentStatus>,
}

impl K8sDeploymentStatus {
    pub fn from_k8s_deployment_status(k8s_deployment_status: DeploymentStatus) -> K8sDeploymentStatus {
        K8sDeploymentStatus {
            replicas: k8s_deployment_status.replicas,
            ready_replicas: k8s_deployment_status.ready_replicas,
        }
    }
}

pub struct K8sDeploymentStatus {
    pub replicas: Option<i32>,
    pub ready_replicas: Option<i32>,
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
                termination_grace_period_seconds: k8s_deployment
                    .metadata
                    .deletion_grace_period_seconds
                    .map(Duration::seconds),
            },
            status: deployment_status,
        })
    }
}

/**********
STATEFULSETS
***********/

pub struct K8sStatefulset {
    pub metadata: K8sMetadata,
    pub status: Option<K8sStatefulsetStatus>,
}

impl K8sStatefulsetStatus {
    pub fn from_k8s_statefulset_status(k8s_statefulset_status: StatefulSetStatus) -> K8sStatefulsetStatus {
        K8sStatefulsetStatus {
            replicas: k8s_statefulset_status.replicas,
            ready_replicas: k8s_statefulset_status.ready_replicas,
        }
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
                termination_grace_period_seconds: k8s_statefulset
                    .metadata
                    .deletion_grace_period_seconds
                    .map(Duration::seconds),
            },
            status: statefulset_status,
        })
    }
}

/**********
SECRETS
***********/

#[derive(Debug)]
pub struct K8sSecret {
    pub metadata: K8sMetadata,
    pub decoded_secret: Option<K8sSecretType>,
}

#[derive(Debug, Clone)]
pub enum K8sSecretType {
    Data(BTreeMap<String, ByteString>),
    StringData(BTreeMap<String, String>),
}

impl K8sSecret {
    pub fn from_k8s_secret_objectlist(event_details: EventDetails, k8s_secrets: ObjectList<Secret>) -> Vec<K8sSecret> {
        let mut secrets: Vec<K8sSecret> = Vec::with_capacity(k8s_secrets.items.len());

        for secret in k8s_secrets.items {
            if let Ok(x) = K8sSecret::from_k8s_secret(event_details.clone(), secret) {
                secrets.push(x);
            };
        }
        secrets
    }

    pub fn from_k8s_secret(event_details: EventDetails, k8s_secret: Secret) -> Result<K8sSecret, Box<EngineError>> {
        let encoded_secret_content = match k8s_secret.data {
            Some(x) => Some(K8sSecretType::Data(x)),
            None => k8s_secret.string_data.map(K8sSecretType::StringData),
        };

        Ok(K8sSecret {
            metadata: K8sMetadata {
                name: match k8s_secret.metadata.name.clone() {
                    Some(x) => x,
                    None => {
                        return Err(Box::new(EngineError::new_k8s_get_secret_error(
                            event_details,
                            CommandError::new_from_safe_message(
                                "can't read kubernetes secret, name is missing".to_string(),
                            ),
                        )))
                    }
                },
                namespace: match k8s_secret.metadata.namespace {
                    Some(x) => x,
                    None => {
                        return Err(Box::new(EngineError::new_k8s_get_secret_error(
                            event_details,
                            CommandError::new_from_safe_message(format!(
                                "can't read kubernetes secret, namespace is missing for deployment name `{}`",
                                k8s_secret.metadata.name.unwrap_or("unknown".to_string())
                            )),
                        )))
                    }
                },
                termination_grace_period_seconds: None,
            },
            decoded_secret: encoded_secret_content,
        })
    }

    pub fn get_decoded_helm_chart_release(
        &self,
        event_details: EventDetails,
    ) -> Result<(ChartReleaseData, String), Box<EngineError>> {
        match self.decoded_secret.clone() {
            Some(K8sSecretType::Data(x)) => {
                // find release key
                match x.iter().find(|(k, _)| k == &&"release".to_string()) {
                    Some(encoded_release) => {
                        let encoded_release = encoded_release.1.clone();
                        // base64 decode release secret
                        let decoded_secret = match base64::decode(encoded_release.0) {
                            Ok(x) => x,
                            Err(e) => {
                                return Err(Box::new(EngineError::new_base64_decode_issue(
                                    event_details,
                                    format!(
                                        "error while decoding secret {}/{}: {}",
                                        self.metadata.namespace, self.metadata.name, e
                                    )
                                    .as_str(),
                                )))
                            }
                        };
                        // gzip uncompress the secret
                        let mut decoder = flate2::read::GzDecoder::new(&decoded_secret[..]);
                        let mut decoded_release = String::new();
                        if let Err(e) = decoder.read_to_string(&mut decoded_release) {
                            return Err(Box::new(EngineError::new_uncompress_issue(
                                event_details,
                                e.to_string().as_str(),
                            )));
                        };
                        let chart: ChartReleaseData = match serde_json::from_str(decoded_release.as_str()) {
                            Ok(x) => x,
                            Err(e) => {
                                return Err(Box::new(EngineError::new_json_serializing_issue(
                                    event_details,
                                    format!("chart release for secret: {e}").as_str(),
                                )))
                            }
                        };
                        Ok((chart, decoded_release))
                    }
                    None => Err(Box::new(EngineError::new_helm_release_data_not_found(
                        event_details,
                        self.metadata.namespace.as_str(),
                        self.metadata.name.as_str(),
                    ))),
                }
            }
            _ => Err(Box::new(EngineError::new_helm_secret_is_missing(
                event_details,
                self.metadata.namespace.as_str(),
                self.metadata.name.as_str(),
            ))),
        }
    }
}

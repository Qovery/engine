use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesList<T> {
    pub items: Vec<T>,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesService {
    pub status: KubernetesServiceStatus,
}

pub struct LabelsContent {
    pub name: String,
    pub value: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata2 {
    pub resource_version: String,
    pub self_link: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub api_version: String,
    pub kind: String,
    pub metadata: Metadata,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    pub creation_timestamp: String,
    pub name: String,
    pub resource_version: String,
    pub self_link: String,
    pub uid: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Daemonset {
    pub api_version: String,
    pub items: Vec<Item>,
    pub kind: String,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesServiceStatus {
    pub load_balancer: KubernetesServiceStatusLoadBalancer,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesServiceStatusLoadBalancer {
    pub ingress: Vec<KubernetesServiceStatusLoadBalancerIngress>,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesServiceStatusLoadBalancerIngress {
    pub hostname: String,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesPod {
    pub status: KubernetesPodStatus,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesPodStatus {
    pub container_statuses: Option<Vec<KubernetesPodContainerStatus>>,
    pub conditions: Vec<KubernetesPodCondition>,
    // read the doc: https://kubernetes.io/docs/concepts/workloads/pods/pod-lifecycle/
    // phase can be Pending, Running, Succeeded, Failed, Unknown
    pub phase: KubernetesPodStatusPhase,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesPodCondition {
    pub status: String,
    #[serde(rename = "type")]
    pub typee: String,
    pub message: Option<String>,
    pub reason: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub enum KubernetesPodStatusPhase {
    Pending,
    Running,
    Succeeded,
    Failed,
    Unknown,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesPodContainerStatus {
    #[serde(rename = "last_state")]
    pub last_state: Option<KubernetesPodContainerStatusLastState>,
    pub ready: bool,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesPodContainerStatusLastState {
    pub terminated: Option<ContainerStatusTerminated>,
    pub waiting: Option<ContainerStatusWaiting>,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct ContainerStatusWaiting {
    pub message: Option<String>,
    pub reason: String,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct ContainerStatusTerminated {
    #[serde(rename = "exit_code")]
    pub exit_code: i16,
    pub message: Option<String>,
    pub reason: String,
    pub signal: i16,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesJob {
    pub status: KubernetesJobStatus,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesJobStatus {
    pub succeeded: u32,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesNode {
    pub status: KubernetesNodeStatus,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesNodeStatus {
    pub allocatable: KubernetesNodeStatusResources,
    pub capacity: KubernetesNodeStatusResources,
    pub node_info: KubernetesNodeInfo,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesNodeStatusResources {
    pub cpu: String,
    pub memory: String,
    pub pods: String,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesNodeInfo {
    pub kube_proxy_version: String,
    pub kubelet_version: String,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesEvent {
    #[serde(rename = "type")]
    pub type_: String,
    pub message: Option<String>,
    pub last_timestamp: Option<String>,
    pub reason: String,
    pub involved_object: KubernetesInvolvedObject,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesInvolvedObject {
    pub kind: String,
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesKind {
    pub kind: String,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesVersion {
    pub server_version: ServerVersion,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerVersion {
    pub major: String,
    pub minor: String,
    pub git_version: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Helm {
    pub name: String,
    pub namespace: String,
    pub revision: String,
    pub updated: String,
    pub status: String,
    pub chart: String,
    #[serde(rename = "app_version")]
    pub app_version: String,
}

pub struct HelmChart {
    pub name: String,
    pub namespace: String,
}

impl HelmChart {
    pub fn new(name: String, namespace: String) -> HelmChart {
        HelmChart { name, namespace }
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct HelmHistoryRow {
    pub revision: u16,
    pub status: String,
    pub chart: String,
    pub app_version: String,
}

impl HelmHistoryRow {
    pub fn is_successfully_deployed(&self) -> bool {
        self.status == "deployed"
    }
}

#[cfg(test)]
mod tests {
    use crate::cmd::structs::{KubernetesList, KubernetesPod};

    #[test]
    fn test_pod_status_deserialize() {
        let payload = r#"
{    "apiVersion": "v1",    "items": [        {            "apiVersion": "v1",            "kind": "Pod",            "metadata": {                "annotations": {                    "kubernetes.io/psp": "eks.privileged"                },                "creationTimestamp": "2021-03-15T15:41:56Z",                "generateName": "postgresqlpostgres-",                "labels": {                    "app": "postgresqlpostgres",                    "chart": "postgresql-8.9.8",                    "controller-revision-hash": "postgresqlpostgres-8db988cfd",                    "heritage": "Helm",                    "release": "postgresql-atx9frcbbrlphzu",                    "role": "master",                    "statefulset.kubernetes.io/pod-name": "postgresqlpostgres-0"                },                "name": "postgresqlpostgres-0",                "namespace": "lbxmwiibzi9lbla-ah5bbhekjarxta5",                "ownerReferences": [                    {                        "apiVersion": "apps/v1",                        "blockOwnerDeletion": true,                        "controller": true,                        "kind": "StatefulSet",                        "name": "postgresqlpostgres",                        "uid": "507ca7da-7d2c-4fdd-90f8-890c8a0d9491"                    }                ],                "resourceVersion": "53444298",                "selfLink": "/api/v1/namespaces/lbxmwiibzi9lbla-ah5bbhekjarxta5/pods/postgresqlpostgres-0",                "uid": "baf9e257-f517-49f5-b530-392a690f5231"            },            "spec": {                "containers": [                    {                        "env": [                            {                                "name": "BITNAMI_DEBUG",                                "value": "false"                            },                            {                                "name": "POSTGRESQL_PORT_NUMBER",                                "value": "5432"                            },                            {                                "name": "POSTGRESQL_VOLUME_DIR",                                "value": "/bitnami/postgresql"                            },                            {                                "name": "POSTGRESQL_INITSCRIPTS_USERNAME",                                "value": "postgres"                            },                            {                                "name": "POSTGRESQL_INITSCRIPTS_PASSWORD",                                "value": "cvbwtt8tzt6jtli"                            },                            {                                "name": "PGDATA",                                "value": "/bitnami/postgresql/data"                            },                            {                                "name": "POSTGRES_POSTGRES_PASSWORD",                                "valueFrom": {                                    "secretKeyRef": {                                        "key": "postgresql-postgres-password",                                        "name": "postgresqlpostgres"                                    }                                }                            },                            {                                "name": "POSTGRES_USER",                                "value": "superuser"                            },                            {                                "name": "POSTGRES_PASSWORD",                                "valueFrom": {                                    "secretKeyRef": {                                        "key": "postgresql-password",                                        "name": "postgresqlpostgres"                                    }                                }                            },                            {                                "name": "POSTGRES_DB",                                "value": "postgres"                            },                            {                                "name": "POSTGRESQL_ENABLE_LDAP",                                "value": "no"                            }                        ],                        "image": "quay.io/bitnami/postgresql:10.16.0",                        "imagePullPolicy": "IfNotPresent",                        "livenessProbe": {                            "exec": {                                "command": [                                    "/bin/sh",                                    "-c",                                    "exec pg_isready -U \"superuser\" -d \"postgres\" -h 127.0.0.1 -p 5432"                                ]                            },                            "failureThreshold": 6,                            "initialDelaySeconds": 30,                            "periodSeconds": 10,                            "successThreshold": 1,                            "timeoutSeconds": 5                        },                        "name": "postgresqlpostgres",                        "ports": [                            {                                "containerPort": 5432,                                "name": "tcp-postgresql",                                "protocol": "TCP"                            }                        ],                        "readinessProbe": {                            "exec": {                                "command": [                                    "/bin/sh",                                    "-c",                                    "-e",                                    "exec pg_isready -U \"superuser\" -d \"postgres\" -h 127.0.0.1 -p 5432\n[ -f /opt/bitnami/postgresql/tmp/.initialized ] || [ -f /bitnami/postgresql/.initialized ]\n"                                ]                            },                            "failureThreshold": 6,                            "initialDelaySeconds": 5,                            "periodSeconds": 10,                            "successThreshold": 1,                            "timeoutSeconds": 5                        },                        "resources": {                            "requests": {                                "cpu": "100m",                                "memory": "50Gi"                            }                        },                        "securityContext": {                            "runAsUser": 1001                        },                        "terminationMessagePath": "/dev/termination-log",                        "terminationMessagePolicy": "File",                        "volumeMounts": [                            {                                "mountPath": "/dev/shm",                                "name": "dshm"                            },                            {                                "mountPath": "/bitnami/postgresql",                                "name": "data"                            },                            {                                "mountPath": "/var/run/secrets/kubernetes.io/serviceaccount",                                "name": "default-token-n6bkr",                                "readOnly": true                            }                        ]                    }                ],                "dnsPolicy": "ClusterFirst",                "enableServiceLinks": true,                "hostname": "postgresqlpostgres-0",                "initContainers": [                    {                        "command": [                            "/bin/sh",                            "-cx",                            "mkdir -p /bitnami/postgresql/data\nchmod 700 /bitnami/postgresql/data\nfind /bitnami/postgresql -mindepth 1 -maxdepth 1 -not -name \"conf\" -not -name \".snapshot\" -not -name \"lost+found\" | \\\n  xargs chown -R 1001:1001\nchmod -R 777 /dev/shm\n"                        ],                        "image": "docker.io/bitnami/minideb:buster",                        "imagePullPolicy": "IfNotPresent",                        "name": "init-chmod-data",                        "resources": {                            "requests": {                                "cpu": "100m",                                "memory": "50Gi"                            }                        },                        "securityContext": {                            "runAsUser": 0                        },                        "terminationMessagePath": "/dev/termination-log",                        "terminationMessagePolicy": "File",                        "volumeMounts": [                            {                                "mountPath": "/bitnami/postgresql",                                "name": "data"                            },                            {                                "mountPath": "/dev/shm",                                "name": "dshm"                            },                            {                                "mountPath": "/var/run/secrets/kubernetes.io/serviceaccount",                                "name": "default-token-n6bkr",                                "readOnly": true                            }                        ]                    }                ],                "priority": 0,                "restartPolicy": "Always",                "schedulerName": "default-scheduler",                "securityContext": {                    "fsGroup": 1001                },                "serviceAccount": "default",                "serviceAccountName": "default",                "subdomain": "postgresqlpostgres-headless",                "terminationGracePeriodSeconds": 30,                "tolerations": [                    {                        "effect": "NoExecute",                        "key": "node.kubernetes.io/not-ready",                        "operator": "Exists",                        "tolerationSeconds": 300                    },                    {                        "effect": "NoExecute",                        "key": "node.kubernetes.io/unreachable",                        "operator": "Exists",                        "tolerationSeconds": 300                    }                ],                "volumes": [                    {                        "name": "data",                        "persistentVolumeClaim": {                            "claimName": "data-postgresqlpostgres-0"                        }                    },                    {                        "emptyDir": {                            "medium": "Memory",                            "sizeLimit": "1Gi"                        },                        "name": "dshm"                    },                    {                        "name": "default-token-n6bkr",                        "secret": {                            "defaultMode": 420,                            "secretName": "default-token-n6bkr"                        }                    }                ]            },            "status": {                "conditions": [                    {                        "lastProbeTime": null,                        "lastTransitionTime": "2021-03-15T15:41:56Z",                        "message": "0/5 nodes are available: 5 Insufficient memory.",                        "reason": "Unschedulable",                        "status": "False",                        "type": "PodScheduled"                    }                ],                "phase": "Pending",                "qosClass": "Burstable"            }        }    ],    "kind": "List",    "metadata": {        "resourceVersion": "",        "selfLink": ""    }}        
        "#;

        let pod_status = serde_json::from_str::<KubernetesList<KubernetesPod>>(payload);
        assert_eq!(pod_status.is_ok(), true);
        assert_eq!(pod_status.unwrap().items[0].status.conditions[0].status, "False");

        let payload = r#"
        {
    "apiVersion": "v1",
    "items": [
        {
            "apiVersion": "v1",
            "kind": "Pod",
            "metadata": {
                "creationTimestamp": "2021-02-26T10:11:37Z",
                "generateName": "gradle-deployment-5654f49c5f-",
                "labels": {
                    "app": "gradle",
                    "pod-template-hash": "5654f49c5f"
                },
                "managedFields": [
                    {
                        "apiVersion": "v1",
                        "fieldsType": "FieldsV1",
                        "fieldsV1": {
                            "f:metadata": {
                                "f:generateName": {},
                                "f:labels": {
                                    ".": {},
                                    "f:app": {},
                                    "f:pod-template-hash": {}
                                },
                                "f:ownerReferences": {
                                    ".": {},
                                    "k:{\"uid\":\"e6c07d77-5b1c-497a-bafa-e24e945dccda\"}": {
                                        ".": {},
                                        "f:apiVersion": {},
                                        "f:blockOwnerDeletion": {},
                                        "f:controller": {},
                                        "f:kind": {},
                                        "f:name": {},
                                        "f:uid": {}
                                    }
                                }
                            },
                            "f:spec": {
                                "f:containers": {
                                    "k:{\"name\":\"gradle\"}": {
                                        ".": {},
                                        "f:args": {},
                                        "f:command": {},
                                        "f:image": {},
                                        "f:imagePullPolicy": {},
                                        "f:name": {},
                                        "f:ports": {
                                            ".": {},
                                            "k:{\"containerPort\":80,\"protocol\":\"TCP\"}": {
                                                ".": {},
                                                "f:containerPort": {},
                                                "f:protocol": {}
                                            }
                                        },
                                        "f:resources": {},
                                        "f:terminationMessagePath": {},
                                        "f:terminationMessagePolicy": {}
                                    }
                                },
                                "f:dnsPolicy": {},
                                "f:enableServiceLinks": {},
                                "f:restartPolicy": {},
                                "f:schedulerName": {},
                                "f:securityContext": {},
                                "f:terminationGracePeriodSeconds": {}
                            }
                        },
                        "manager": "kube-controller-manager",
                        "operation": "Update",
                        "time": "2021-02-26T10:11:37Z"
                    },
                    {
                        "apiVersion": "v1",
                        "fieldsType": "FieldsV1",
                        "fieldsV1": {
                            "f:status": {
                                "f:conditions": {
                                    "k:{\"type\":\"ContainersReady\"}": {
                                        ".": {},
                                        "f:lastProbeTime": {},
                                        "f:lastTransitionTime": {},
                                        "f:status": {},
                                        "f:type": {}
                                    },
                                    "k:{\"type\":\"Initialized\"}": {
                                        ".": {},
                                        "f:lastProbeTime": {},
                                        "f:lastTransitionTime": {},
                                        "f:status": {},
                                        "f:type": {}
                                    },
                                    "k:{\"type\":\"Ready\"}": {
                                        ".": {},
                                        "f:lastProbeTime": {},
                                        "f:lastTransitionTime": {},
                                        "f:status": {},
                                        "f:type": {}
                                    }
                                },
                                "f:containerStatuses": {},
                                "f:hostIP": {},
                                "f:phase": {},
                                "f:podIP": {},
                                "f:podIPs": {
                                    ".": {},
                                    "k:{\"ip\":\"10.244.0.68\"}": {
                                        ".": {},
                                        "f:ip": {}
                                    }
                                },
                                "f:startTime": {}
                            }
                        },
                        "manager": "kubelet",
                        "operation": "Update",
                        "time": "2021-02-26T10:11:43Z"
                    }
                ],
                "name": "gradle-deployment-5654f49c5f-dw8zl",
                "namespace": "default",
                "ownerReferences": [
                    {
                        "apiVersion": "apps/v1",
                        "blockOwnerDeletion": true,
                        "controller": true,
                        "kind": "ReplicaSet",
                        "name": "gradle-deployment-5654f49c5f",
                        "uid": "e6c07d77-5b1c-497a-bafa-e24e945dccda"
                    }
                ],
                "resourceVersion": "9095811",
                "selfLink": "/api/v1/namespaces/default/pods/gradle-deployment-5654f49c5f-dw8zl",
                "uid": "c10f29f2-35d6-42dc-b9e8-71c99d7123e2"
            },
            "spec": {
                "containers": [
                    {
                        "args": [
                            "-c",
                            "sleep 6000000"
                        ],
                        "command": [
                            "/bin/sh"
                        ],
                        "image": "ubuntu:latest",
                        "imagePullPolicy": "IfNotPresent",
                        "name": "gradle",
                        "ports": [
                            {
                                "containerPort": 80,
                                "protocol": "TCP"
                            }
                        ],
                        "resources": {},
                        "terminationMessagePath": "/dev/termination-log",
                        "terminationMessagePolicy": "File",
                        "volumeMounts": [
                            {
                                "mountPath": "/var/run/secrets/kubernetes.io/serviceaccount",
                                "name": "default-token-p85k2",
                                "readOnly": true
                            }
                        ]
                    }
                ],
                "dnsPolicy": "ClusterFirst",
                "enableServiceLinks": true,
                "imagePullSecrets": [
                    {
                        "name": "default-docr-registry-qovery-do-test"
                    }
                ],
                "nodeName": "qovery-gqgyb7zy4ykwumak-3zl08",
                "priority": 0,
                "restartPolicy": "Always",
                "schedulerName": "default-scheduler",
                "securityContext": {},
                "serviceAccount": "default",
                "serviceAccountName": "default",
                "terminationGracePeriodSeconds": 30,
                "tolerations": [
                    {
                        "effect": "NoExecute",
                        "key": "node.kubernetes.io/not-ready",
                        "operator": "Exists",
                        "tolerationSeconds": 300
                    },
                    {
                        "effect": "NoExecute",
                        "key": "node.kubernetes.io/unreachable",
                        "operator": "Exists",
                        "tolerationSeconds": 300
                    }
                ],
                "volumes": [
                    {
                        "name": "default-token-p85k2",
                        "secret": {
                            "defaultMode": 420,
                            "secretName": "default-token-p85k2"
                        }
                    }
                ]
            },
            "status": {
                "conditions": [
                    {
                        "lastProbeTime": null,
                        "lastTransitionTime": "2021-02-26T10:11:37Z",
                        "status": "True",
                        "type": "Initialized"
                    },
                    {
                        "lastProbeTime": null,
                        "lastTransitionTime": "2021-02-26T10:11:43Z",
                        "status": "True",
                        "type": "Ready"
                    },
                    {
                        "lastProbeTime": null,
                        "lastTransitionTime": "2021-02-26T10:11:43Z",
                        "status": "True",
                        "type": "ContainersReady"
                    },
                    {
                        "lastProbeTime": null,
                        "lastTransitionTime": "2021-02-26T10:11:37Z",
                        "status": "True",
                        "type": "PodScheduled"
                    }
                ],
                "containerStatuses": [
                    {
                        "containerID": "docker://3afa93048e28f823becac70f17546a6bd7d83a8c50c25e22b8c0a1ca6b91aa21",
                        "image": "ubuntu:latest",
                        "imageID": "docker-pullable://ubuntu@sha256:703218c0465075f4425e58fac086e09e1de5c340b12976ab9eb8ad26615c3715",
                        "lastState": {},
                        "name": "gradle",
                        "ready": true,
                        "restartCount": 0,
                        "started": true,
                        "state": {
                            "running": {
                                "startedAt": "2021-02-26T10:11:42Z"
                            }
                        }
                    }
                ],
                "hostIP": "10.20.0.3",
                "phase": "Running",
                "podIP": "10.244.0.68",
                "podIPs": [
                    {
                        "ip": "10.244.0.68"
                    }
                ],
                "qosClass": "BestEffort",
                "startTime": "2021-02-26T10:11:37Z"
            }
        }
    ],
    "kind": "List",
    "metadata": {
        "resourceVersion": "",
        "selfLink": ""
    }
}
        "#;

        let pod_status = serde_json::from_str::<KubernetesList<KubernetesPod>>(payload);
        assert_eq!(pod_status.is_ok(), true);
    }
}

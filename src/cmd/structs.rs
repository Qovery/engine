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

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Labels {
    pub name: String,
}

pub struct LabelsContent {
    pub name: String,
    pub value: String,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spec {
    pub finalizers: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub phase: String,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata2 {
    pub resource_version: String,
    pub self_link: String,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub api_version: String,
    pub kind: String,
    pub metadata: Metadata,
    pub spec: Spec,
    pub status: Status,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    pub creation_timestamp: String,
    pub labels: Option<Labels>,
    pub name: String,
    pub resource_version: String,
    pub self_link: String,
    pub uid: String,
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
    pub container_statuses: Vec<KubernetesPodContainerStatus>,
    // read the doc: https://kubernetes.io/docs/concepts/workloads/pods/pod-lifecycle/
    // phase can be Pending, Running, Succeeded, Failed, Unknown
    pub phase: KubernetesPodStatusPhase,
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
pub struct KubernetesEvent {
    #[serde(rename = "type")]
    pub type_: String,
    pub message: Option<String>,
    pub reason: String,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
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

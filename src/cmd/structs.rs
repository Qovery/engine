use crate::cmd::structs::KubernetesPodStatusReason::Unknown;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Deserialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesList<T> {
    pub items: Vec<T>,
}

#[derive(Deserialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesService {
    pub status: KubernetesServiceStatus,
}

#[derive(Deserialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesIngress {
    pub status: KubernetesIngressStatus,
}

pub struct LabelsContent {
    pub name: String,
    pub value: String,
}

#[derive(Default, Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Secrets {
    pub api_version: String,
    pub kind: String,
    pub metadata: SecretsMetadata,
    pub items: Vec<SecretItem>,
}

#[derive(Default, Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretsMetadata {
    pub resource_version: String,
    pub self_link: String,
}

#[derive(Default, Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretItem {
    pub api_version: String,
    pub kind: String,
    pub metadata: SecretMetadata,
    pub data: HashMap<String, String>,
}

#[derive(Default, Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretMetadata {
    pub creation_timestamp: String,
    pub name: String,
    pub resource_version: String,
    pub uid: String,
}

#[derive(Default, Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub api_version: String,
    pub kind: String,
    pub metadata: ItemMetadata,
}

#[derive(Default, Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemMetadata {
    pub creation_timestamp: String,
    pub name: String,
    pub resource_version: String,
    #[serde(default)]
    pub self_link: String,
    pub uid: String,
    pub annotations: HashMap<String, String>,
}

#[derive(Default, Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Namespace {
    pub api_version: String,
    pub kind: String,
    pub metadata: NamespaceMetadata,
}

#[derive(Default, Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NamespaceMetadata {
    pub creation_timestamp: String,
    pub name: String,
    pub resource_version: String,
    pub uid: String,
}

#[derive(Deserialize)]
pub struct Configmap {
    pub data: ConfigmapData,
}

#[derive(Hash, Deserialize)]
pub struct ConfigmapData {
    #[serde(rename = "Corefile")]
    pub corefile: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Daemonset {
    pub api_version: String,
    pub items: Option<Vec<Item>>,
    pub kind: String,
    pub spec: Option<Spec>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spec {
    pub selector: Selector,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Selector {
    pub match_labels: MatchLabels,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchLabels {
    #[serde(rename = "k8s-app")]
    pub k8s_app: Option<String>,
}

#[derive(Deserialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesServiceStatus {
    pub load_balancer: KubernetesServiceStatusLoadBalancer,
}

#[derive(Deserialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesServiceStatusLoadBalancer {
    pub ingress: Vec<KubernetesServiceStatusLoadBalancerIngress>,
}

#[derive(Deserialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesServiceStatusLoadBalancerIngress {
    pub hostname: String,
}

#[derive(Deserialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesIngressStatus {
    pub load_balancer: KubernetesIngressStatusLoadBalancer,
}

#[derive(Deserialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesIngressStatusLoadBalancer {
    pub ingress: Vec<KubernetesIngressStatusLoadBalancerIngress>,
}

#[derive(Deserialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesIngressStatusLoadBalancerIngress {
    pub ip: Option<String>,
    pub hostname: Option<String>,
}

#[derive(Deserialize, Clone, Eq, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesPod {
    pub status: KubernetesPodStatus,
    pub metadata: KubernetesPodMetadata,
}

#[derive(Deserialize, Clone, Eq, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesPodMetadata {
    pub name: String,
    pub namespace: String,
}

#[derive(Deserialize, Clone, Eq, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesPodStatus {
    pub container_statuses: Option<Vec<KubernetesPodContainerStatus>>,
    pub conditions: Option<Vec<KubernetesPodCondition>>,
    // read the doc: https://kubernetes.io/docs/concepts/workloads/pods/pod-lifecycle/
    // phase can be Pending, Running, Succeeded, Failed, Unknown
    pub phase: KubernetesPodStatusPhase,
}

#[derive(Deserialize, Clone, Eq, PartialEq, Debug)]
#[serde(rename_all = "PascalCase", from = "String")]
/// KubernetesPodStatusReason: Details about why the pod is in this state. e.g. 'Evicted'
/// https://github.com/kubernetes/kubernetes/blob/master/pkg/kubelet/events/event.go#L20
pub enum KubernetesPodStatusReason {
    Unknown(Option<String>),
    Created,
    Started,
    Failed,
    Killing,
    Preempting,
    CrashLoopBackOff,
    ExceededGracePeriod,
}

impl Default for KubernetesPodStatusReason {
    fn default() -> Self {
        Unknown(None)
    }
}

impl From<String> for KubernetesPodStatusReason {
    fn from(s: String) -> Self {
        match s.to_lowercase().as_str() {
            "created" => KubernetesPodStatusReason::Created,
            "started" => KubernetesPodStatusReason::Started,
            "failed" => KubernetesPodStatusReason::Failed,
            "killing" => KubernetesPodStatusReason::Killing,
            "preempting" => KubernetesPodStatusReason::Preempting,
            "crashloopbackoff" => KubernetesPodStatusReason::CrashLoopBackOff,
            "exceededgraceperiod" => KubernetesPodStatusReason::ExceededGracePeriod,
            _ => Unknown(match s.as_str() {
                "" => None,
                _ => Some(s),
            }),
        }
    }
}

#[derive(Deserialize, Clone, Eq, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesPodCondition {
    pub status: String,
    #[serde(rename = "type")]
    pub typee: String,
    pub message: Option<String>,
    #[serde(default)]
    pub reason: KubernetesPodStatusReason,
}

#[derive(Deserialize, Clone, Eq, PartialEq, Debug)]
pub enum KubernetesPodStatusPhase {
    Pending,
    Running,
    Succeeded,
    Failed,
    Unknown,
}

#[derive(Deserialize, Clone, Eq, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesPodContainerStatus {
    pub last_state: Option<KubernetesPodContainerStatusState>,
    pub state: KubernetesPodContainerStatusState,
    pub ready: bool,
    pub restart_count: usize,
}

#[derive(Deserialize, Clone, Eq, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesPodContainerStatusState {
    pub terminated: Option<ContainerStatusTerminated>,
    pub waiting: Option<ContainerStatusWaiting>,
}

#[derive(Deserialize, Clone, Eq, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ContainerStatusWaiting {
    pub message: Option<String>,
    #[serde(default)]
    pub reason: KubernetesPodStatusReason,
}

#[derive(Deserialize, Clone, Eq, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ContainerStatusTerminated {
    pub exit_code: i16,
    pub message: Option<String>,
    #[serde(default)]
    pub reason: KubernetesPodStatusReason,
}

#[derive(Deserialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesJob {
    pub status: KubernetesJobStatus,
}

#[derive(Deserialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesJobStatus {
    pub succeeded: u32,
}

#[derive(Deserialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesNode {
    pub status: KubernetesNodeStatus,
}

#[derive(Deserialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesNodeStatus {
    pub allocatable: KubernetesNodeStatusResources,
    pub capacity: KubernetesNodeStatusResources,
    pub node_info: KubernetesNodeInfo,
    pub conditions: Vec<KubernetesNodeCondition>,
}

#[derive(Deserialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesNodeStatusResources {
    pub cpu: String,
    pub memory: String,
    pub pods: String,
}

#[derive(Deserialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesNodeInfo {
    pub kube_proxy_version: String,
    pub kubelet_version: String,
}

#[derive(Deserialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesNodeCondition {
    #[serde(rename = "type")]
    pub condition_type: String,
    pub status: String,
}

#[derive(Deserialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesEvent {
    #[serde(rename = "type")]
    pub type_: String,
    pub message: Option<String>,
    pub last_timestamp: Option<String>,
    pub reason: String,
    pub involved_object: KubernetesInvolvedObject,
}

#[derive(Deserialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesInvolvedObject {
    pub kind: String,
    pub name: String,
}

#[derive(Deserialize, Clone, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesKind {
    pub kind: String,
}

#[derive(Default, Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesVersion {
    pub server_version: ServerVersion,
}

#[derive(Default, Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerVersion {
    pub major: String,
    pub minor: String,
    pub git_version: String,
}

#[derive(Default, Debug, Clone, PartialEq, Deserialize)]
pub struct HelmListItem {
    pub name: String,
    pub namespace: String,
    pub revision: String,
    pub updated: String,
    pub status: String,
    pub chart: String,
    pub app_version: String,
}

#[derive(Clone, PartialEq)]
pub struct HelmChart {
    pub name: String,
    pub namespace: String,
    pub chart_version: Option<Version>,
    pub app_version: Option<Version>,
}

#[derive(Clone, PartialEq)]
pub struct HelmChartVersions {
    pub chart_version: Option<Version>,
    pub app_version: Option<Version>,
}

impl HelmChart {
    pub fn new(
        name: String,
        namespace: String,
        chart_version: Option<Version>,
        app_version: Option<Version>,
    ) -> HelmChart {
        HelmChart {
            name,
            namespace,
            chart_version,
            app_version,
        }
    }
}

#[derive(Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct HelmHistoryRow {
    pub revision: u16,
    pub updated: String,
    pub status: String,
    pub chart: String,
    pub app_version: String,
}

impl HelmHistoryRow {
    pub fn is_successfully_deployed(&self) -> bool {
        self.status == "deployed"
    }
}

#[derive(Default, Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PVC {
    pub api_version: String,
    pub items: Option<Vec<PVCItem>>,
    pub kind: String,
}

#[derive(Default, Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PVCItem {
    pub api_version: String,
    pub kind: String,
    pub metadata: PVCMetadata,
    pub spec: PVCSpec,
}

#[derive(Default, Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PVCMetadata {
    pub resource_version: String,
}

#[derive(Default, Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PVCSpec {
    pub access_modes: Option<Vec<String>>,
    pub resources: PVCResources,
    pub storage_class_name: String,
    pub volume_mode: String,
    pub volume_name: String,
}

#[derive(Default, Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PVCResources {
    pub requests: PVCRequests,
}

#[derive(Default, Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PVCRequests {
    pub storage: String,
}

#[derive(Default, Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SVC {
    pub api_version: String,
    pub items: Option<Vec<SVCItem>>,
    pub kind: String,
}

#[derive(Default, Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SVCItem {
    pub api_version: String,
    pub kind: String,
    pub metadata: SVCMetadata,
    pub spec: SVCSpec,
}

#[derive(Default, Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SVCMetadata {
    pub resource_version: String,
    pub name: String,
    pub annotations: HashMap<String, String>,
}

#[derive(Default, Debug, Clone, PartialEq, Deserialize)]
pub struct SVCSpec {
    #[serde(rename = "type")]
    pub svc_type: String,
}

#[derive(Default, Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PDB {
    pub api_version: String,
    pub items: Option<Vec<PDBItem>>,
    pub kind: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PDBMetadata {
    pub name: String,
    pub namespace: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PDBItem {
    pub api_version: String,
    pub kind: String,
    pub status: PDBStatus,
    pub metadata: PDBMetadata,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PDBStatus {
    pub current_healthy: i16,
    pub desired_healthy: i16,
    pub disruptions_allowed: i16,
    pub expected_pods: i16,
    pub observed_generation: i16,
}

#[derive(Default, Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HPA {
    pub api_version: String,
    pub items: Option<Vec<HPAItem>>,
    pub kind: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HPAItem {
    pub api_version: String,
    pub kind: String,
    pub metadata: HPAMetadata,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HPAMetadata {
    pub annotations: Option<HPAAnnotationCondition>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HPAAnnotationCondition {
    #[serde(rename = "autoscaling.alpha.kubernetes.io/conditions")]
    pub conditions: Option<String>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsServer {
    pub kind: String,
    pub api_version: String,
    pub name: String,
}

#[derive(Deserialize, Clone, Eq, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesDeployment {
    pub kind: String,
}

#[derive(Deserialize, Clone, Eq, PartialEq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesStatefulSet {
    pub kind: String,
}

#[cfg(test)]
mod tests {
    use crate::cmd::structs::{KubernetesList, KubernetesPod, KubernetesPodStatusReason, MetricsServer, PDB, PVC, SVC};

    #[test]
    fn test_svc_deserialize() {
        // setup:
        let payload = r#"{
            "apiVersion": "v1",
            "kind": "Service",
            "metadata": {
                "annotations": {
                    "meta.helm.sh/release-name": "application-z164e3ad8-z164e3ad8",
                    "meta.helm.sh/release-namespace": "z9b830e28-ze23976e2"
                },
                "creationTimestamp": "2021-11-30T09:08:52Z",
                "labels": {
                    "app": "app-z164e3ad8",
                    "app.kubernetes.io/managed-by": "Helm",
                    "appId": "z164e3ad8",
                    "envId": "ze23976e2",
                    "ownerId": "FAKE"
                },
                "name": "app-z164e3ad8",
                "namespace": "z9b830e28-ze23976e2",
                "resourceVersion": "6801889",
                "uid": "c165f1b0-b372-449e-9ffa-ed2f06fee7c3"
            },
            "spec": {
                "clusterIP": "10.245.19.143",
                "ports": [
                    {
                        "name": "p80",
                        "port": 80,
                        "protocol": "TCP",
                        "targetPort": 80
                    }
                ],
                "selector": {
                    "app": "app-z164e3ad8",
                    "appId": "z164e3ad8",
                    "envId": "ze23976e2",
                    "ownerId": "FAKE"
                },
                "sessionAffinity": "None",
                "type": "ClusterIP"
            },
            "status": {
                "loadBalancer": {}
            }
        }"#;

        // execute:
        let svc = serde_json::from_str::<SVC>(payload);

        // verify:
        match svc {
            Ok(_) => {
                // OK
            }
            Err(e) => {
                panic!("Panic ! Error: {}", e)
            }
        }
    }

    #[test]
    fn test_pvc_deserialize() {
        // setup:
        let payload = r#"{
  "apiVersion": "v1",
  "items": [
    {
      "apiVersion": "v1",
      "kind": "PersistentVolumeClaim",
      "metadata": {
        "annotations": {
          "pv.kubernetes.io/bind-completed": "yes",
          "pv.kubernetes.io/bound-by-controller": "yes",
          "volume.beta.kubernetes.io/storage-provisioner": "csi.scaleway.com",
          "volume.kubernetes.io/selected-node": "scw-qovery-z093e29e2-z093e29e2-1-672f4a75df734"
        },
        "creationTimestamp": "2021-12-16T15:05:28Z",
        "finalizers": [
          "kubernetes.io/pvc-protection"
        ],
        "labels": {
          "app": "app-simple-app-vsxgtriudbloeaa",
          "appId": "ri5j3sycsocnadf",
          "diskId": "wx3s3f67pruykgz",
          "diskType": "scw-sbv-ssd-0",
          "envId": "ezpiedcfaxmxexz",
          "ownerId": "ibokvref94rpp0p"
        },
        "managedFields": [
          {
            "apiVersion": "v1",
            "fieldsType": "FieldsV1",
            "fieldsV1": {
              "f:metadata": {
                "f:annotations": {
                  "f:pv.kubernetes.io/bind-completed": {},
                  "f:pv.kubernetes.io/bound-by-controller": {},
                  "f:volume.beta.kubernetes.io/storage-provisioner": {}
                },
                "f:labels": {
                  ".": {},
                  "f:app": {},
                  "f:appId": {},
                  "f:diskId": {},
                  "f:diskType": {},
                  "f:envId": {},
                  "f:ownerId": {}
                }
              },
              "f:spec": {
                "f:accessModes": {},
                "f:resources": {
                  "f:requests": {
                    ".": {},
                    "f:storage": {}
                  }
                },
                "f:storageClassName": {},
                "f:volumeMode": {},
                "f:volumeName": {}
              },
              "f:status": {
                "f:accessModes": {},
                "f:capacity": {
                  ".": {},
                  "f:storage": {}
                },
                "f:phase": {}
              }
            },
            "manager": "kube-controller-manager",
            "operation": "Update",
            "time": "2021-12-16T15:05:28Z"
          },
          {
            "apiVersion": "v1",
            "fieldsType": "FieldsV1",
            "fieldsV1": {
              "f:metadata": {
                "f:annotations": {
                  ".": {},
                  "f:volume.kubernetes.io/selected-node": {}
                }
              }
            },
            "manager": "kube-scheduler",
            "operation": "Update",
            "time": "2021-12-16T15:05:28Z"
          }
        ],
        "name": "wx3s3f67pruykgz-app-simple-app-vsxgtriudbloeaa-0",
        "namespace": "kzaqt7x0ylvtcic-ezpiedcfaxmxexz",
        "resourceVersion": "895119134",
        "uid": "6c881b93-c580-4121-a846-6352cc75c991"
      },
      "spec": {
        "accessModes": [
          "ReadWriteOnce"
        ],
        "resources": {
          "requests": {
            "storage": "10Gi"
          }
        },
        "storageClassName": "scw-sbv-ssd-0",
        "volumeMode": "Filesystem",
        "volumeName": "pvc-6c881b93-c580-4121-a846-6352cc75c991"
      },
      "status": {
        "accessModes": [
          "ReadWriteOnce"
        ],
        "capacity": {
          "storage": "10Gi"
        },
        "phase": "Bound"
      }
    },
    {
      "apiVersion": "v1",
      "kind": "PersistentVolumeClaim",
      "metadata": {
        "annotations": {
          "pv.kubernetes.io/bind-completed": "yes",
          "pv.kubernetes.io/bound-by-controller": "yes",
          "volume.beta.kubernetes.io/storage-provisioner": "csi.scaleway.com",
          "volume.kubernetes.io/selected-node": "scw-qovery-z093e29e2-z093e29e2-1-672f4a75df734"
        },
        "creationTimestamp": "2021-12-16T15:07:00Z",
        "finalizers": [
          "kubernetes.io/pvc-protection"
        ],
        "labels": {
          "app": "app-simple-app-vsxgtriudbloeaa",
          "appId": "ri5j3sycsocnadf",
          "diskId": "wx3s3f67pruykgz",
          "diskType": "scw-sbv-ssd-0",
          "envId": "ezpiedcfaxmxexz",
          "ownerId": "ibokvref94rpp0p"
        },
        "managedFields": [
          {
            "apiVersion": "v1",
            "fieldsType": "FieldsV1",
            "fieldsV1": {
              "f:metadata": {
                "f:annotations": {
                  "f:pv.kubernetes.io/bind-completed": {},
                  "f:pv.kubernetes.io/bound-by-controller": {},
                  "f:volume.beta.kubernetes.io/storage-provisioner": {}
                },
                "f:labels": {
                  ".": {},
                  "f:app": {},
                  "f:appId": {},
                  "f:diskId": {},
                  "f:diskType": {},
                  "f:envId": {},
                  "f:ownerId": {}
                }
              },
              "f:spec": {
                "f:accessModes": {},
                "f:resources": {
                  "f:requests": {
                    ".": {},
                    "f:storage": {}
                  }
                },
                "f:storageClassName": {},
                "f:volumeMode": {},
                "f:volumeName": {}
              },
              "f:status": {
                "f:accessModes": {},
                "f:capacity": {
                  ".": {},
                  "f:storage": {}
                },
                "f:phase": {}
              }
            },
            "manager": "kube-controller-manager",
            "operation": "Update",
            "time": "2021-12-16T15:07:00Z"
          },
          {
            "apiVersion": "v1",
            "fieldsType": "FieldsV1",
            "fieldsV1": {
              "f:metadata": {
                "f:annotations": {
                  ".": {},
                  "f:volume.kubernetes.io/selected-node": {}
                }
              }
            },
            "manager": "kube-scheduler",
            "operation": "Update",
            "time": "2021-12-16T15:07:00Z"
          }
        ],
        "name": "wx3s3f67pruykgz-app-simple-app-vsxgtriudbloeaa-1",
        "namespace": "kzaqt7x0ylvtcic-ezpiedcfaxmxexz",
        "resourceVersion": "895134137",
        "uid": "b92b653f-6a4e-40c3-a16e-7e0c9701df3e"
      },
      "spec": {
        "accessModes": [
          "ReadWriteOnce"
        ],
        "resources": {
          "requests": {
            "storage": "10Gi"
          }
        },
        "storageClassName": "scw-sbv-ssd-0",
        "volumeMode": "Filesystem",
        "volumeName": "pvc-b92b653f-6a4e-40c3-a16e-7e0c9701df3e"
      },
      "status": {
        "accessModes": [
          "ReadWriteOnce"
        ],
        "capacity": {
          "storage": "10Gi"
        },
        "phase": "Bound"
      }
    }
  ],
  "kind": "List",
  "metadata": {
    "resourceVersion": "",
    "selfLink": ""
  }
}"#;

        // execute:
        let pvc = serde_json::from_str::<PVC>(payload);

        // verify:
        match pvc {
            Ok(_) => {
                // OK
            }
            Err(e) => {
                panic!("Panic ! Error: {}", e)
            }
        }
    }

    #[test]
    fn test_pod_status_deserialize() {
        let payload = r#"{
  "apiVersion": "v1",
  "items": [
    {
      "apiVersion": "v1",
      "kind": "Pod",
      "metadata": {
        "annotations": {
          "kubernetes.io/psp": "eks.privileged"
        },
        "creationTimestamp": "2021-03-15T15:41:56Z",
        "generateName": "postgresqlpostgres-",
        "labels": {
          "app": "postgresqlpostgres",
          "chart": "postgresql-8.9.8",
          "controller-revision-hash": "postgresqlpostgres-8db988cfd",
          "heritage": "Helm",
          "release": "postgresql-atx9frcbbrlphzu",
          "role": "master",
          "statefulset.kubernetes.io/pod-name": "postgresqlpostgres-0"
        },
        "name": "postgresqlpostgres-0",
        "namespace": "lbxmwiibzi9lbla-ah5bbhekjarxta5",
        "ownerReferences": [
          {
            "apiVersion": "apps/v1",
            "blockOwnerDeletion": true,
            "controller": true,
            "kind": "StatefulSet",
            "name": "postgresqlpostgres",
            "uid": "507ca7da-7d2c-4fdd-90f8-890c8a0d9491"
          }
        ],
        "resourceVersion": 53444298,
        "selfLink": "/api/v1/namespaces/lbxmwiibzi9lbla-ah5bbhekjarxta5/pods/postgresqlpostgres-0",
        "uid": "baf9e257-f517-49f5-b530-392a690f5231"
      },
      "spec": {
        "containers": [
          {
            "env": [
              {
                "name": "BITNAMI_DEBUG",
                "value": false
              }
            ],
            "image": "docker.io/bitnami/postgresql:10.16.0",
            "imagePullPolicy": "IfNotPresent",
            "livenessProbe": {
              "exec": {
                "command": [
                  "/bin/sh",
                  "-c",
                  "exec pg_isready -U \"superuser\" -d \"postgres\" -h 127.0.0.1 -p 5432"
                ]
              },
              "failureThreshold": 6,
              "initialDelaySeconds": 30,
              "periodSeconds": 10,
              "successThreshold": 1,
              "timeoutSeconds": 5
            },
            "name": "postgresqlpostgres",
            "ports": [
              {
                "containerPort": 5432,
                "name": "tcp-postgresql",
                "protocol": "TCP"
              }
            ],
            "readinessProbe": {
              "exec": {
                "command": [
                  "/bin/sh",
                  "-c",
                  "-e",
                  "exec pg_isready -U \"superuser\" -d \"postgres\" -h 127.0.0.1 -p 5432\n[ -f /opt/bitnami/postgresql/tmp/.initialized ] || [ -f /bitnami/postgresql/.initialized ]\n"
                ]
              },
              "failureThreshold": 6,
              "initialDelaySeconds": 5,
              "periodSeconds": 10,
              "successThreshold": 1,
              "timeoutSeconds": 5
            },
            "resources": {
              "requests": {
                "cpu": "100m",
                "memory": "50Gi"
              }
            },
            "securityContext": {
              "runAsUser": 1001
            },
            "terminationMessagePath": "/dev/termination-log",
            "terminationMessagePolicy": "File",
            "volumeMounts": [
              {
                "mountPath": "/dev/shm",
                "name": "dshm"
              },
              {
                "mountPath": "/bitnami/postgresql",
                "name": "data"
              },
              {
                "mountPath": "/var/run/secrets/kubernetes.io/serviceaccount",
                "name": "default-token-n6bkr",
                "readOnly": true
              }
            ]
          }
        ],
        "dnsPolicy": "ClusterFirst",
        "enableServiceLinks": true,
        "hostname": "postgresqlpostgres-0",
        "initContainers": [
          {
            "command": [
              "/bin/sh",
              "-cx",
              "mkdir -p /bitnami/postgresql/data\nchmod 700 /bitnami/postgresql/data\nfind /bitnami/postgresql -mindepth 1 -maxdepth 1 -not -name \"conf\" -not -name \".snapshot\" -not -name \"lost+found\" | \\\n  xargs chown -R 1001:1001\nchmod -R 777 /dev/shm\n"
            ],
            "image": "docker.io/bitnami/minideb:buster",
            "imagePullPolicy": "IfNotPresent",
            "name": "init-chmod-data",
            "resources": {
              "requests": {
                "cpu": "100m",
                "memory": "50Gi"
              }
            },
            "securityContext": {
              "runAsUser": 0
            },
            "terminationMessagePath": "/dev/termination-log",
            "terminationMessagePolicy": "File",
            "volumeMounts": [
              {
                "mountPath": "/bitnami/postgresql",
                "name": "data"
              },
              {
                "mountPath": "/dev/shm",
                "name": "dshm"
              },
              {
                "mountPath": "/var/run/secrets/kubernetes.io/serviceaccount",
                "name": "default-token-n6bkr",
                "readOnly": true
              }
            ]
          }
        ],
        "priority": 0,
        "restartPolicy": "Always",
        "schedulerName": "default-scheduler",
        "securityContext": {
          "fsGroup": 1001
        },
        "serviceAccount": "default",
        "serviceAccountName": "default",
        "subdomain": "postgresqlpostgres-headless",
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
            "name": "data",
            "persistentVolumeClaim": {
              "claimName": "data-postgresqlpostgres-0"
            }
          },
          {
            "emptyDir": {
              "medium": "Memory",
              "sizeLimit": "1Gi"
            },
            "name": "dshm"
          },
          {
            "name": "default-token-n6bkr",
            "secret": {
              "defaultMode": 420,
              "secretName": "default-token-n6bkr"
            }
          }
        ]
      },
      "status": {
        "conditions": [
          {
            "lastProbeTime": null,
            "lastTransitionTime": "2021-03-15T15:41:56Z",
            "message": "0/5 nodes are available: 5 Insufficient memory.",
            "reason": "Unschedulable",
            "status": "False",
            "type": "PodScheduled"
          }
        ],
        "phase": "Pending",
        "qosClass": "Burstable"
      }
    }
  ],
  "kind": "List",
  "metadata": {
    "resourceVersion": "",
    "selfLink": ""
  }
}"#;

        let pod_status = serde_json::from_str::<KubernetesList<KubernetesPod>>(payload);
        assert!(pod_status.is_ok());
        let pod_status = pod_status.unwrap();
        assert_eq!(pod_status.items[0].status.conditions.as_ref().unwrap()[0].status, "False");
        assert_eq!(
            pod_status.items[0].status.conditions.as_ref().unwrap()[0].reason,
            KubernetesPodStatusReason::Unknown(Some("Unschedulable".to_string()))
        );

        let payload = r#"{
  "apiVersion": "v1",
  "items": [
    {
      "apiVersion": "v1",
      "kind": "Pod",
      "metadata": {
        "annotations": {
          "kubernetes.io/psp": "eks.privileged"
        },
        "creationTimestamp": "2021-03-15T15:41:56Z",
        "generateName": "postgresqlpostgres-",
        "labels": {
          "app": "postgresqlpostgres",
          "chart": "postgresql-8.9.8",
          "controller-revision-hash": "postgresqlpostgres-8db988cfd",
          "heritage": "Helm",
          "release": "postgresql-atx9frcbbrlphzu",
          "role": "master",
          "statefulset.kubernetes.io/pod-name": "postgresqlpostgres-0"
        },
        "name": "postgresqlpostgres-0",
        "namespace": "lbxmwiibzi9lbla-ah5bbhekjarxta5",
        "ownerReferences": [
          {
            "apiVersion": "apps/v1",
            "blockOwnerDeletion": true,
            "controller": true,
            "kind": "StatefulSet",
            "name": "postgresqlpostgres",
            "uid": "507ca7da-7d2c-4fdd-90f8-890c8a0d9491"
          }
        ],
        "resourceVersion": 53444298,
        "selfLink": "/api/v1/namespaces/lbxmwiibzi9lbla-ah5bbhekjarxta5/pods/postgresqlpostgres-0",
        "uid": "baf9e257-f517-49f5-b530-392a690f5231"
      },
      "spec": {
        "containers": [
          {
            "env": [
              {
                "name": "BITNAMI_DEBUG",
                "value": false
              }
            ],
            "image": "docker.io/bitnami/postgresql:10.16.0",
            "imagePullPolicy": "IfNotPresent",
            "livenessProbe": {
              "exec": {
                "command": [
                  "/bin/sh",
                  "-c",
                  "exec pg_isready -U \"superuser\" -d \"postgres\" -h 127.0.0.1 -p 5432"
                ]
              },
              "failureThreshold": 6,
              "initialDelaySeconds": 30,
              "periodSeconds": 10,
              "successThreshold": 1,
              "timeoutSeconds": 5
            },
            "name": "postgresqlpostgres",
            "ports": [
              {
                "containerPort": 5432,
                "name": "tcp-postgresql",
                "protocol": "TCP"
              }
            ],
            "readinessProbe": {
              "exec": {
                "command": [
                  "/bin/sh",
                  "-c",
                  "-e",
                  "exec pg_isready -U \"superuser\" -d \"postgres\" -h 127.0.0.1 -p 5432\n[ -f /opt/bitnami/postgresql/tmp/.initialized ] || [ -f /bitnami/postgresql/.initialized ]\n"
                ]
              },
              "failureThreshold": 6,
              "initialDelaySeconds": 5,
              "periodSeconds": 10,
              "successThreshold": 1,
              "timeoutSeconds": 5
            },
            "resources": {
              "requests": {
                "cpu": "100m",
                "memory": "50Gi"
              }
            },
            "securityContext": {
              "runAsUser": 1001
            },
            "terminationMessagePath": "/dev/termination-log",
            "terminationMessagePolicy": "File",
            "volumeMounts": [
              {
                "mountPath": "/dev/shm",
                "name": "dshm"
              },
              {
                "mountPath": "/bitnami/postgresql",
                "name": "data"
              },
              {
                "mountPath": "/var/run/secrets/kubernetes.io/serviceaccount",
                "name": "default-token-n6bkr",
                "readOnly": true
              }
            ]
          }
        ],
        "dnsPolicy": "ClusterFirst",
        "enableServiceLinks": true,
        "hostname": "postgresqlpostgres-0",
        "initContainers": [
          {
            "command": [
              "/bin/sh",
              "-cx",
              "mkdir -p /bitnami/postgresql/data\nchmod 700 /bitnami/postgresql/data\nfind /bitnami/postgresql -mindepth 1 -maxdepth 1 -not -name \"conf\" -not -name \".snapshot\" -not -name \"lost+found\" | \\\n  xargs chown -R 1001:1001\nchmod -R 777 /dev/shm\n"
            ],
            "image": "docker.io/bitnami/minideb:buster",
            "imagePullPolicy": "IfNotPresent",
            "name": "init-chmod-data",
            "resources": {
              "requests": {
                "cpu": "100m",
                "memory": "50Gi"
              }
            },
            "securityContext": {
              "runAsUser": 0
            },
            "terminationMessagePath": "/dev/termination-log",
            "terminationMessagePolicy": "File",
            "volumeMounts": [
              {
                "mountPath": "/bitnami/postgresql",
                "name": "data"
              },
              {
                "mountPath": "/dev/shm",
                "name": "dshm"
              },
              {
                "mountPath": "/var/run/secrets/kubernetes.io/serviceaccount",
                "name": "default-token-n6bkr",
                "readOnly": true
              }
            ]
          }
        ],
        "priority": 0,
        "restartPolicy": "Always",
        "schedulerName": "default-scheduler",
        "securityContext": {
          "fsGroup": 1001
        },
        "serviceAccount": "default",
        "serviceAccountName": "default",
        "subdomain": "postgresqlpostgres-headless",
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
            "name": "data",
            "persistentVolumeClaim": {
              "claimName": "data-postgresqlpostgres-0"
            }
          },
          {
            "emptyDir": {
              "medium": "Memory",
              "sizeLimit": "1Gi"
            },
            "name": "dshm"
          },
          {
            "name": "default-token-n6bkr",
            "secret": {
              "defaultMode": 420,
              "secretName": "default-token-n6bkr"
            }
          }
        ]
      },
      "status": {
        "conditions": [
          {
            "lastProbeTime": null,
            "lastTransitionTime": "2021-03-15T15:41:56Z",
            "message": "0/5 nodes are available: 5 Insufficient memory.",
            "reason": "CrashLoopBackOff",
            "status": "False",
            "type": "PodScheduled"
          }
        ],
        "phase": "Pending",
        "qosClass": "Burstable"
      }
    }
  ],
  "kind": "List",
  "metadata": {
    "resourceVersion": "",
    "selfLink": ""
  }
}"#;

        let pod_status = serde_json::from_str::<KubernetesList<KubernetesPod>>(payload);
        assert!(pod_status.is_ok());
        let pod_status = pod_status.unwrap();
        assert_eq!(pod_status.items[0].status.conditions.as_ref().unwrap()[0].status, "False");
        assert_eq!(
            pod_status.items[0].status.conditions.as_ref().unwrap()[0].reason,
            KubernetesPodStatusReason::CrashLoopBackOff
        );

        let payload = r#"{
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
    }}"#;

        let pod_status = serde_json::from_str::<KubernetesList<KubernetesPod>>(payload);

        assert!(pod_status.is_ok());
        assert_eq!(
            pod_status.unwrap().items[0].status.conditions.as_ref().unwrap()[0].reason,
            KubernetesPodStatusReason::Unknown(None)
        );

        let pod_status = r#"
{
    "apiVersion": "v1",
    "kind": "Pod",
    "metadata": {
        "annotations": {
            "appCommitId": "9b1baf132923531777268e9f0335d9dc8a1a9fb5",
            "checksum/config": "67a36e7db6bafd338ae37c69332e24be7096944df98a1685b92ab4269c5d3536",
            "kubernetes.io/psp": "eks.privileged"
        },
        "creationTimestamp": "2022-07-13T14:33:21Z",
        "generateName": "app-z064fc389-55c7689767-",
        "labels": {
            "app": "app-z064fc389",
            "appId": "z064fc389",
            "appLongId": "064fc389-e9cc-4957-b071-6b040dbe3b8b",
            "envId": "zdf4a84bc",
            "envLongId": "df4a84bc-e80f-4be3-9279-aa6ffb30a9a0",
            "ownerId": "FAKE",
            "pod-template-hash": "55c7689767",
            "projectLongId": "801e6dcb-ab22-41ea-86d1-8daf31624202"
        },
        "name": "app-z064fc389-55c7689767-9bchb",
        "namespace": "z801e6dcb-zdf4a84bc",
        "ownerReferences": [
            {
                "apiVersion": "apps/v1",
                "blockOwnerDeletion": true,
                "controller": true,
                "kind": "ReplicaSet",
                "name": "app-z064fc389-55c7689767",
                "uid": "283106ed-b459-444e-b13d-45a6fc9955ff"
            }
        ],
        "resourceVersion": "6023822",
        "uid": "216693e7-9179-40b7-b5ec-41ecabbec634"
    },
    "spec": {
        "affinity": {
            "podAntiAffinity": {
                "requiredDuringSchedulingIgnoredDuringExecution": [
                    {
                        "labelSelector": {
                            "matchExpressions": [
                                {
                                    "key": "app",
                                    "operator": "In",
                                    "values": [
                                        "app-z064fc389"
                                    ]
                                }
                            ]
                        },
                        "topologyKey": "kubernetes.io/hostname"
                    }
                ]
            }
        },
        "automountServiceAccountToken": false,
        "containers": [
            {
                "env": [
                    {
                        "name": "ALGOLIA_ID",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "ALGOLIA_ID",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "ALGOLIA_KEY",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "ALGOLIA_KEY",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "AWS_ACCESS_KEY_ID",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "AWS_ACCESS_KEY_ID",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "AWS_REGION",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "AWS_REGION",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "AWS_SECRET_ACCESS_KEY",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "AWS_SECRET_ACCESS_KEY",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "BUDGET_INSIGHT_API_URL",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "BUDGET_INSIGHT_API_URL",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "BUDGET_INSIGHT_CLIENT_ID",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "BUDGET_INSIGHT_CLIENT_ID",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "BUDGET_INSIGHT_CLIENT_SECRET",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "BUDGET_INSIGHT_CLIENT_SECRET",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "CALENDLY_API_TOKEN",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "CALENDLY_API_TOKEN",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "CALENDLY_CALENDAR_URL",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "CALENDLY_CALENDAR_URL",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "CALENDLY_ORGANIZATION_URI",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "CALENDLY_ORGANIZATION_URI",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "CALENDLY_WEBHOOK_TOKEN",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "CALENDLY_WEBHOOK_TOKEN",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "ELASTIC_CLOUD_URL",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "ELASTIC_CLOUD_URL",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "FRONT_HOSTNAME",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "FRONT_HOSTNAME",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "GIT_SECRET",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "GIT_SECRET",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "GOOGLE_MAP_API_KEY",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "GOOGLE_MAP_API_KEY",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "HOSTNAME",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "HOSTNAME",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "HTTP_BASIC_AUTH_PASS",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "HTTP_BASIC_AUTH_PASS",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "HTTP_BASIC_AUTH_USER",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "HTTP_BASIC_AUTH_USER",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "IBAN_VALIDATOR_ID",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "IBAN_VALIDATOR_ID",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "IBAN_VALIDATOR_PWD",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "IBAN_VALIDATOR_PWD",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "IBAN_VALIDATOR_URL",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "IBAN_VALIDATOR_URL",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "KICKBOX_API_KEY",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "KICKBOX_API_KEY",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "MAILCHIMP_API_KEY",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "MAILCHIMP_API_KEY",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "MAILCHIMP_MAIN_LIST_ID",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "MAILCHIMP_MAIN_LIST_ID",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "MAILCHIMP_PROSPECT_LIST_ID",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "MAILCHIMP_PROSPECT_LIST_ID",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "MAILCHIMP_VERIFY",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "MAILCHIMP_VERIFY",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "MANDRILL_API_KEY",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "MANDRILL_API_KEY",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "NEXMO_API_KEY",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "NEXMO_API_KEY",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "NEXMO_API_SECRET",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "NEXMO_API_SECRET",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "PAPERTRAIL_API_TOKEN",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "PAPERTRAIL_API_TOKEN",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "POSTGRESQL_ENV_DATABASE",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "POSTGRESQL_ENV_DATABASE",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "POSTGRESQL_ENV_POSTGRES_HOST",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "POSTGRESQL_ENV_POSTGRES_HOST",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "POSTGRESQL_ENV_POSTGRES_PASSWORD",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "POSTGRESQL_ENV_POSTGRES_PASSWORD",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "POSTGRESQL_ENV_POSTGRES_USER",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "POSTGRESQL_ENV_POSTGRES_USER",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "QOVERY_APPLICATION_Z064FC389_ENVIRONMENT_NAME",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "QOVERY_APPLICATION_Z064FC389_ENVIRONMENT_NAME",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "QOVERY_APPLICATION_Z064FC389_GIT_BRANCH",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "QOVERY_APPLICATION_Z064FC389_GIT_BRANCH",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "QOVERY_APPLICATION_Z064FC389_GIT_COMMIT_ID",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "QOVERY_APPLICATION_Z064FC389_GIT_COMMIT_ID",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "QOVERY_APPLICATION_Z064FC389_HOST_EXTERNAL",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "QOVERY_APPLICATION_Z064FC389_HOST_EXTERNAL",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "QOVERY_APPLICATION_Z064FC389_HOST_INTERNAL",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "QOVERY_APPLICATION_Z064FC389_HOST_INTERNAL",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "QOVERY_APPLICATION_Z064FC389_NAME",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "QOVERY_APPLICATION_Z064FC389_NAME",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "QOVERY_APPLICATION_Z22E5806D_HOST_EXTERNAL",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "QOVERY_APPLICATION_Z22E5806D_HOST_EXTERNAL",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "QOVERY_APPLICATION_Z22E5806D_HOST_INTERNAL",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "QOVERY_APPLICATION_Z22E5806D_HOST_INTERNAL",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "QOVERY_APPLICATION_Z4BCC3D9A_HOST_EXTERNAL",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "QOVERY_APPLICATION_Z4BCC3D9A_HOST_EXTERNAL",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "QOVERY_APPLICATION_Z4BCC3D9A_HOST_INTERNAL",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "QOVERY_APPLICATION_Z4BCC3D9A_HOST_INTERNAL",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "QOVERY_APPLICATION_ZC1096A84_HOST_INTERNAL",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "QOVERY_APPLICATION_ZC1096A84_HOST_INTERNAL",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "QOVERY_ENVIRONMENT_ID",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "QOVERY_ENVIRONMENT_ID",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "QOVERY_POSTGRESQL_Z909E13C8_DATABASE_URL",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "QOVERY_POSTGRESQL_Z909E13C8_DATABASE_URL",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "QOVERY_POSTGRESQL_Z909E13C8_DATABASE_URL_INTERNAL",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "QOVERY_POSTGRESQL_Z909E13C8_DATABASE_URL_INTERNAL",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "QOVERY_POSTGRESQL_Z909E13C8_DEFAULT_DATABASE_NAME",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "QOVERY_POSTGRESQL_Z909E13C8_DEFAULT_DATABASE_NAME",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "QOVERY_POSTGRESQL_Z909E13C8_HOST",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "QOVERY_POSTGRESQL_Z909E13C8_HOST",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "QOVERY_POSTGRESQL_Z909E13C8_HOST_INTERNAL",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "QOVERY_POSTGRESQL_Z909E13C8_HOST_INTERNAL",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "QOVERY_POSTGRESQL_Z909E13C8_LOGIN",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "QOVERY_POSTGRESQL_Z909E13C8_LOGIN",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "QOVERY_POSTGRESQL_Z909E13C8_PASSWORD",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "QOVERY_POSTGRESQL_Z909E13C8_PASSWORD",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "QOVERY_POSTGRESQL_Z909E13C8_PORT",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "QOVERY_POSTGRESQL_Z909E13C8_PORT",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "QOVERY_PROJECT_ID",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "QOVERY_PROJECT_ID",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "QOVERY_REDIS_ZCDDA3D69_DATABASE_URL",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "QOVERY_REDIS_ZCDDA3D69_DATABASE_URL",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "QOVERY_REDIS_ZCDDA3D69_DATABASE_URL_INTERNAL",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "QOVERY_REDIS_ZCDDA3D69_DATABASE_URL_INTERNAL",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "QOVERY_REDIS_ZCDDA3D69_DEFAULT_DATABASE_NAME",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "QOVERY_REDIS_ZCDDA3D69_DEFAULT_DATABASE_NAME",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "QOVERY_REDIS_ZCDDA3D69_HOST",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "QOVERY_REDIS_ZCDDA3D69_HOST",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "QOVERY_REDIS_ZCDDA3D69_HOST_INTERNAL",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "QOVERY_REDIS_ZCDDA3D69_HOST_INTERNAL",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "QOVERY_REDIS_ZCDDA3D69_LOGIN",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "QOVERY_REDIS_ZCDDA3D69_LOGIN",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "QOVERY_REDIS_ZCDDA3D69_PASSWORD",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "QOVERY_REDIS_ZCDDA3D69_PASSWORD",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "QOVERY_REDIS_ZCDDA3D69_PORT",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "QOVERY_REDIS_ZCDDA3D69_PORT",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "RACK_ALLOW_ORIGIN",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "RACK_ALLOW_ORIGIN",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "RAILS_ENV",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "RAILS_ENV",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "REDIS_HOST",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "REDIS_HOST",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "REDIS_PASSWORD",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "REDIS_PASSWORD",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "SALESFORCE_ACCOUNT_ID",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "SALESFORCE_ACCOUNT_ID",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "SALESFORCE_API_VERSION",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "SALESFORCE_API_VERSION",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "SALESFORCE_CLIENT_ID",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "SALESFORCE_CLIENT_ID",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "SALESFORCE_CLIENT_SECRET",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "SALESFORCE_CLIENT_SECRET",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "SALESFORCE_HOST",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "SALESFORCE_HOST",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "SALESFORCE_PASSWORD",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "SALESFORCE_PASSWORD",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "SALESFORCE_SECURITY_TOKEN",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "SALESFORCE_SECURITY_TOKEN",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "SALESFORCE_USERNAME",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "SALESFORCE_USERNAME",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "SECRET_KEY_BASE",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "SECRET_KEY_BASE",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "SETUP_DATABASE",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "SETUP_DATABASE",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "SIDEKIQ_CLIENT_SIZE",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "SIDEKIQ_CLIENT_SIZE",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "SIDEKIQ_SERVER_SIZE",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "SIDEKIQ_SERVER_SIZE",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "TACOGUESS_PASSWORD",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "TACOGUESS_PASSWORD",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "TACOGUESS_URL",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "TACOGUESS_URL",
                                "name": "app-z064fc389"
                            }
                        }
                    },
                    {
                        "name": "TACOGUESS_USERNAME",
                        "valueFrom": {
                            "secretKeyRef": {
                                "key": "TACOGUESS_USERNAME",
                                "name": "app-z064fc389"
                            }
                        }
                    }
                ],
                "image": "335433052139.dkr.ecr.eu-west-3.amazonaws.com/z064fc389:17791480101993828140-9b1baf132923531777268e9f0335d9dc8a1a9fb5",
                "imagePullPolicy": "IfNotPresent",
                "livenessProbe": {
                    "failureThreshold": 3,
                    "initialDelaySeconds": 30,
                    "periodSeconds": 10,
                    "successThreshold": 1,
                    "tcpSocket": {
                        "port": 3000
                    },
                    "timeoutSeconds": 5
                },
                "name": "app-z064fc389",
                "ports": [
                    {
                        "containerPort": 3000,
                        "name": "p3000",
                        "protocol": "TCP"
                    }
                ],
                "readinessProbe": {
                    "failureThreshold": 3,
                    "initialDelaySeconds": 30,
                    "periodSeconds": 10,
                    "successThreshold": 1,
                    "tcpSocket": {
                        "port": 3000
                    },
                    "timeoutSeconds": 1
                },
                "resources": {
                    "limits": {
                        "cpu": "500m",
                        "memory": "1Gi"
                    },
                    "requests": {
                        "cpu": "500m",
                        "memory": "1Gi"
                    }
                },
                "terminationMessagePath": "/dev/termination-log",
                "terminationMessagePolicy": "File"
            }
        ],
        "dnsPolicy": "ClusterFirst",
        "enableServiceLinks": true,
        "imagePullSecrets": [
            {
                "name": "335433052139.dkr.ecr.eu-west-3.amazonaws.com"
            }
        ],
        "nodeName": "ip-10-0-21-229.eu-west-3.compute.internal",
        "preemptionPolicy": "PreemptLowerPriority",
        "priority": 0,
        "restartPolicy": "Always",
        "schedulerName": "default-scheduler",
        "securityContext": {},
        "serviceAccount": "default",
        "serviceAccountName": "default",
        "terminationGracePeriodSeconds": 60,
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
        ]
    },
    "status": {
        "message": "The node was low on resource: ephemeral-storage. Container app-z064fc389 was using 56Ki, which exceeds its request of 0. ",
        "phase": "Failed",
        "reason": "Evicted",
        "startTime": "2022-07-13T14:34:59Z"
    }
}
        "#;

        let pod_status = serde_json::from_str::<KubernetesPod>(pod_status);
        assert!(pod_status.is_ok());
    }

    #[test]
    fn test_pdb_deserialize() {
        // setup:
        let payload = r#"{
    "apiVersion": "v1",
    "items": [
        {
            "apiVersion": "policy/v1beta1",
            "kind": "PodDisruptionBudget",
            "metadata": {
                "annotations": {
                    "kubectl.kubernetes.io/last-applied-configuration": "{\"apiVersion\":\"policy/v1beta1\",\"kind\":\"PodDisruptionBudget\",\"metadata\":{\"annotations\":{},\"labels\":{\"io.cilium/app\":\"operator\",\"k8s.scw.cloud/cni\":\"cilium\",\"k8s.scw.cloud/object\":\"PodDisruptionBudget\",\"k8s.scw.cloud/system\":\"cni\",\"name\":\"cilium-operator\"},\"name\":\"cilium-operator\",\"namespace\":\"kube-system\"},\"spec\":{\"maxUnavailable\":1,\"selector\":{\"matchLabels\":{\"io.cilium/app\":\"operator\",\"name\":\"cilium-operator\"}}}}\n"
                },
                "creationTimestamp": "2021-10-21T09:35:38Z",
                "generation": 1,
                "labels": {
                    "io.cilium/app": "operator",
                    "k8s.scw.cloud/cni": "cilium",
                    "k8s.scw.cloud/object": "PodDisruptionBudget",
                    "k8s.scw.cloud/system": "cni",
                    "name": "cilium-operator"
                },
                "name": "cilium-operator",
                "namespace": "kube-system",
                "resourceVersion": "878978452",
                "uid": "1941df75-a535-4138-9bf9-865cf69f5519"
            },
            "spec": {
                "maxUnavailable": 1,
                "selector": {
                    "matchLabels": {
                        "io.cilium/app": "operator",
                        "name": "cilium-operator"
                    }
                }
            },
            "status": {
                "currentHealthy": 1,
                "desiredHealthy": 0,
                "disruptionsAllowed": 1,
                "expectedPods": 1,
                "observedGeneration": 1
            }
        },
        {
            "apiVersion": "policy/v1beta1",
            "kind": "PodDisruptionBudget",
            "metadata": {
                "annotations": {
                    "meta.helm.sh/release-name": "qovery-engine",
                    "meta.helm.sh/release-namespace": "qovery"
                },
                "creationTimestamp": "2021-11-29T13:10:34Z",
                "generation": 1,
                "labels": {
                    "app.kubernetes.io/instance": "qovery-engine",
                    "app.kubernetes.io/managed-by": "Helm",
                    "app.kubernetes.io/name": "qovery-engine",
                    "app.kubernetes.io/version": "0.1.0",
                    "helm.sh/chart": "qovery-engine-0.1.0"
                },
                "name": "qovery-engine",
                "namespace": "qovery",
                "resourceVersion": "948768849",
                "uid": "a2798d0b-7f66-469c-84de-2778ab39048a"
            },
            "spec": {
                "minAvailable": "50%",
                "selector": {
                    "matchLabels": {
                        "app.kubernetes.io/instance": "qovery-engine"
                    }
                }
            },
            "status": {
                "currentHealthy": 2,
                "desiredHealthy": 1,
                "disruptionsAllowed": 1,
                "expectedPods": 2,
                "observedGeneration": 1
            }
        },
        {
            "apiVersion": "policy/v1beta1",
            "kind": "PodDisruptionBudget",
            "metadata": {
                "annotations": {
                    "meta.helm.sh/release-name": "application-z584b6585-z584b6585",
                    "meta.helm.sh/release-namespace": "za2730025-z18650490"
                },
                "creationTimestamp": "2021-12-16T09:30:57Z",
                "generation": 1,
                "labels": {
                    "app": "app-z584b6585",
                    "app.kubernetes.io/managed-by": "Helm",
                    "appId": "z584b6585",
                    "envId": "z18650490",
                    "ownerId": "FAKE"
                },
                "name": "app-z584b6585",
                "namespace": "za2730025-z18650490",
                "resourceVersion": "892065755",
                "uid": "ec7c8f98-3cf2-4b77-b5c1-4e449a12be51"
            },
            "spec": {
                "minAvailable": 1,
                "selector": {
                    "matchLabels": {
                        "app": "app-z584b6585",
                        "appId": "z584b6585",
                        "envId": "z18650490",
                        "ownerId": "FAKE"
                    }
                }
            },
            "status": {
                "currentHealthy": 1,
                "desiredHealthy": 1,
                "disruptionsAllowed": 0,
                "expectedPods": 1,
                "observedGeneration": 1
            }
        },
        {
            "apiVersion": "policy/v1beta1",
            "kind": "PodDisruptionBudget",
            "metadata": {
                "annotations": {
                    "meta.helm.sh/release-name": "application-z3644afeb-z3644afeb",
                    "meta.helm.sh/release-namespace": "zf5a85953-z1dc0c973"
                },
                "creationTimestamp": "2021-12-20T13:58:45Z",
                "generation": 1,
                "labels": {
                    "app": "app-z3644afeb",
                    "app.kubernetes.io/managed-by": "Helm",
                    "appId": "z3644afeb",
                    "envId": "z1dc0c973",
                    "ownerId": "FAKE"
                },
                "name": "app-z3644afeb",
                "namespace": "zf5a85953-z1dc0c973",
                "resourceVersion": "959451320",
                "uid": "7ea75ab3-4a1f-401e-a0e8-11203ae621e9"
            },
            "spec": {
                "minAvailable": 1,
                "selector": {
                    "matchLabels": {
                        "app": "app-z3644afeb",
                        "appId": "z3644afeb",
                        "envId": "z1dc0c973",
                        "ownerId": "FAKE"
                    }
                }
            },
            "status": {
                "currentHealthy": 0,
                "desiredHealthy": 1,
                "disruptionsAllowed": 0,
                "expectedPods": 1,
                "observedGeneration": 1
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

        // execute:
        let pdb = serde_json::from_str::<PDB>(payload);

        // verify:
        match pdb {
            Ok(_) => {
                // OK
            }
            Err(e) => {
                panic!("Panic ! Error: {}", e)
            }
        }
    }

    #[test]
    fn test_metrics_server_deserialize() {
        // setup:
        let payload = r#"{"kind":"APIGroup","apiVersion":"v1","name":"metrics.k8s.io","versions":[{"groupVersion":"metrics.k8s.io/v1beta1","version":"v1beta1"}],"preferredVersion":{"groupVersion":"metrics.k8s.io/v1beta1","version":"v1beta1"}}"#;

        // execute:
        let metrics_server = serde_json::from_str::<MetricsServer>(payload);

        // verify:
        match metrics_server {
            Ok(_) => {
                // OK
            }
            Err(e) => {
                panic!("Panic ! Error: {}", e)
            }
        }
    }
}

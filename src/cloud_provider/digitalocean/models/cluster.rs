#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Clusters {
    #[serde(rename = "kubernetes_clusters")]
    pub kubernetes_clusters: Vec<KubernetesCluster>,
    pub meta: Option<Meta>,
    pub links: Option<Links>,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cluster {
    #[serde(rename = "kubernetes_cluster")]
    pub kubernetes_cluster: KubernetesCluster,
}


#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesCluster {
    pub id: String,
    pub name: String,
    pub region: String,
    pub version: String,
    #[serde(rename = "cluster_subnet")]
    pub cluster_subnet: String,
    #[serde(rename = "service_subnet")]
    pub service_subnet: String,
    #[serde(rename = "vpc_uuid")]
    pub vpc_uuid: String,
    pub ipv4: String,
    pub endpoint: String,
    pub tags: Vec<String>,
    #[serde(rename = "node_pools")]
    pub node_pools: Vec<NodePool>,
    #[serde(rename = "maintenance_policy")]
    pub maintenance_policy: MaintenancePolicy,
    #[serde(rename = "auto_upgrade")]
    pub auto_upgrade: bool,
    pub status: Status2,
    #[serde(rename = "created_at")]
    pub created_at: String,
    #[serde(rename = "updated_at")]
    pub updated_at: String,
    #[serde(rename = "surge_upgrade")]
    pub surge_upgrade: bool,
    #[serde(rename = "registry_enabled")]
    pub registry_enabled: bool,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodePool {
    pub id: String,
    pub name: String,
    pub size: String,
    pub count: i64,
    pub tags: Vec<String>,
    pub labels: ::serde_json::Value,
    pub taints: Vec<::serde_json::Value>,
    #[serde(rename = "auto_scale")]
    pub auto_scale: bool,
    #[serde(rename = "min_nodes")]
    pub min_nodes: i64,
    #[serde(rename = "max_nodes")]
    pub max_nodes: i64,
    pub nodes: Option<Vec<Node>>,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub id: String,
    pub name: String,
    pub status: Status,
    #[serde(rename = "droplet_id")]
    pub droplet_id: String,
    #[serde(rename = "created_at")]
    pub created_at: String,
    #[serde(rename = "updated_at")]
    pub updated_at: String,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub state: String,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaintenancePolicy {
    #[serde(rename = "start_time")]
    pub start_time: String,
    pub duration: String,
    pub day: String,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status2 {
    pub state: String,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    pub total: i64,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Links {}

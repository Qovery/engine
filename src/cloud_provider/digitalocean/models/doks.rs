use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct DoksList {
    pub kubernetes_clusters: Vec<KubernetesCluster>,
}

#[derive(Default, Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct KubernetesCluster {
    pub id: String,
    pub name: String,
    pub region: String,
    pub version: String,
    pub cluster_subnet: String,
    pub service_subnet: String,
    pub vpc_uuid: String,
}

#[derive(Default, Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub struct DoksOptions {
    pub options: Options,
}

#[derive(Default, Serialize, Deserialize, PartialEq, Debug)]
pub struct Options {
    pub versions: Vec<KubernetesVersion>,
}

#[derive(Default, Serialize, Deserialize, PartialEq, Debug)]
pub struct KubernetesVersion {
    pub slug: String,
    pub kubernetes_version: String,
}

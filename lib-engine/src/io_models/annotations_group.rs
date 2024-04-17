use serde_derive::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash, Debug)]
pub struct AnnotationsGroup {
    #[serde(default)]
    pub annotations: Vec<Annotation>,
    #[serde(default)]
    pub scopes: Vec<AnnotationsGroupScope>,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash, Debug)]
pub struct Annotation {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub value: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AnnotationsGroupScope {
    Deployments,
    StatefulSets,
    Services,
    Ingress,
    Hpa,
    Pods,
    Secrets,
    Jobs,
    CronJobs,
    #[serde(other)]
    Unknown,
}

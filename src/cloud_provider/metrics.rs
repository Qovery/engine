use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct KubernetesApiMetrics {
    pub items: Vec<MetricValue>,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct MetricValue {
    pub value: String,
}

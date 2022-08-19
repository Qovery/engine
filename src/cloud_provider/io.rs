use crate::cloud_provider::Kind as KindModel;
use serde_derive::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Aws,
    Do,
    Scw,
}

impl From<KindModel> for Kind {
    fn from(kind: KindModel) -> Self {
        match kind {
            KindModel::Aws => Kind::Aws,
            KindModel::Do => Kind::Do,
            KindModel::Scw => Kind::Scw,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct ClusterAdvancedSettings {
    #[serde(alias = "load_balancer.size")]
    pub load_balancer_size: String,
    #[serde(alias = "registry.image_retention_time")]
    pub registry_image_retention_time: u32,
    #[serde(alias = "pleco.resources.ttl")]
    pub pleco_resources_ttl: i32,
}

impl Default for ClusterAdvancedSettings {
    fn default() -> Self {
        ClusterAdvancedSettings {
            load_balancer_size: "lb-s".to_string(),
            registry_image_retention_time: 31536000,
            pleco_resources_ttl: -1,
        }
    }
}

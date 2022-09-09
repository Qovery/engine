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
    pub registry_image_retention_time_sec: u32,
    #[serde(alias = "pleco.resources_ttl")]
    pub pleco_resources_ttl: i32,
    #[serde(alias = "loki.log_retention_in_week")]
    pub loki_log_retention_in_week: u32,
    #[serde(alias = "aws.iam.admin_group")]
    pub aws_iam_user_mapper_group_name: String,
}

impl Default for ClusterAdvancedSettings {
    fn default() -> Self {
        ClusterAdvancedSettings {
            load_balancer_size: "lb-s".to_string(),
            registry_image_retention_time_sec: 31536000,
            pleco_resources_ttl: -1,
            loki_log_retention_in_week: 12,
            aws_iam_user_mapper_group_name: "Admins".to_string(),
        }
    }
}

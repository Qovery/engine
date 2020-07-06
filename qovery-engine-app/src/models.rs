use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct Request {
    pub created_at: DateTime<Utc>,
    pub action: Action,
    pub build_platform: BuildPlatform,
}

#[derive(Serialize, Deserialize, Clone)]
pub enum Action {
    Create,
    Delete,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct BuildPlatform {
    pub kind: qovery_engine::build_platform::Kind,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CloudProvider {
    pub kind: qovery_engine::cloud_provider::Kind,
    pub kubernetes: Kubernetes,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Kubernetes {
    pub kind: qovery_engine::cloud_provider::kubernetes::Kind,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ContainerRegistry {
    pub kind: qovery_engine::container_registry::Kind,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Response {
    pub created_at: DateTime<Utc>,
    pub message: Option<String>,
}

impl Response {
    pub fn new(message: Option<String>) -> Self {
        Response {
            created_at: Utc::now(),
            message,
        }
    }

    pub fn as_json_string(&self) -> String {
        serde_json::to_string(self).unwrap()
    }
}

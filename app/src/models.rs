use crate::task_manager::task_manager::Status;
use chrono::{DateTime, Utc};

#[derive(Clone, Copy, Debug)]
pub enum TaskSelector {
    Infrastructure(&'static str),
    Environment(&'static str),
}

#[derive(Serialize, Deserialize)]
pub struct StatusResponse {
    id: String,
    created_at: DateTime<Utc>,
    status: Status,
}

impl StatusResponse {
    pub fn new(id: String, status: Status) -> Self {
        StatusResponse {
            id,
            created_at: Utc::now(),
            status,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Ping {
    created_at: DateTime<Utc>,
    engine_started_at: DateTime<Utc>,
    engine_name: String,
    // TODO add stats? deployments: { total, total_successes, total_failed ...}
}

impl Ping {
    pub fn new(engine_started_at: DateTime<Utc>, engine_name: &str) -> Self {
        Ping {
            created_at: Utc::now(),
            engine_started_at,
            engine_name: engine_name.to_string(),
        }
    }
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

// #[derive(Serialize, Deserialize, Clone)]
// pub struct LoadBalanceTaskRequest {
//     pub group_id: String,
//     pub created_at: DateTime<Utc>,
// }
//
// impl LoadBalanceTaskRequest {
//     pub fn new(group_id: String, created_at: DateTime<Utc>) -> Self {
//         LoadBalanceTaskRequest {
//             group_id,
//             created_at,
//         }
//     }
//
//     pub fn as_json_string(&self) -> String {
//         serde_json::to_string(self).unwrap()
//     }
// }

// #[derive(Serialize, Deserialize, Clone)]
// pub struct LoadBalanceTaskResponse {
//     pub is_first_place: bool,
// }
//
// impl LoadBalanceTaskResponse {
//     pub fn new(is_first_place: bool) -> Self {
//         LoadBalanceTaskResponse { is_first_place }
//     }
//
//     pub fn as_json_string(&self) -> String {
//         serde_json::to_string(self).unwrap()
//     }
// }

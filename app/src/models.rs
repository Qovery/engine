use chrono::{DateTime, Utc};
use qovery_engine_task_manager::task_manager::Status;

#[derive(Clone, Copy)]
pub enum TaskSelector {
    Infrastructure(&'static str),
    Environment(&'static str),
}

impl TaskSelector {
    pub fn name(&self) -> &'static str {
        match self {
            TaskSelector::Infrastructure(_) => "infrastructure",
            TaskSelector::Environment(_) => "environment",
        }
    }
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

#[derive(Serialize, Deserialize, Clone)]
pub struct CheckTaskRunningResponse {
    pub is_running: bool,
}

impl CheckTaskRunningResponse {
    pub fn new(is_running: bool) -> Self {
        CheckTaskRunningResponse { is_running }
    }

    pub fn as_json_string(&self) -> String {
        serde_json::to_string(self).unwrap()
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CheckTaskOrderRequest {
    pub group_id: String,
    pub created_at: DateTime<Utc>,
}

impl CheckTaskOrderRequest {
    pub fn new(group_id: String, created_at: DateTime<Utc>) -> Self {
        CheckTaskOrderRequest {
            group_id,
            created_at,
        }
    }

    pub fn as_json_string(&self) -> String {
        serde_json::to_string(self).unwrap()
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CheckTaskOrderResponse {
    pub is_first_place: bool,
}

impl CheckTaskOrderResponse {
    pub fn new(is_first_place: bool) -> Self {
        CheckTaskOrderResponse { is_first_place }
    }

    pub fn as_json_string(&self) -> String {
        serde_json::to_string(self).unwrap()
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct GetTaskManagerInfoRequest<'a> {
    pub requester_engine_id: &'a str,
}

impl<'a> GetTaskManagerInfoRequest<'a> {
    pub fn new(requester_engine_id: &'a str) -> Self {
        GetTaskManagerInfoRequest {
            requester_engine_id,
        }
    }

    pub fn as_json_string(&self) -> String {
        serde_json::to_string(self).unwrap()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GetTaskManagerInfoResponse {
    pub engine_id: String,
    pub incoming_task_subject_name: String,
    pub remaining_tasks_to_run: usize,
}

impl GetTaskManagerInfoResponse {
    pub fn new<T: Into<String>>(
        engine_id: T,
        incoming_task_subject_name: T,
        remaining_tasks_to_run: usize,
    ) -> Self {
        GetTaskManagerInfoResponse {
            engine_id: engine_id.into(),
            incoming_task_subject_name: incoming_task_subject_name.into(),
            remaining_tasks_to_run,
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

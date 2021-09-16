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

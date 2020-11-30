use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct WorkerNodeDataTemplate {
    pub instance_type: String,
    pub desired_size: String,
    pub max_size: String,
    pub min_size: String,
}

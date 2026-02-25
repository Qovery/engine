use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;

/// DTO layer - abstract deserialization from JSON input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretsManagerAccessDto {
    pub id: String,
    pub endpoint: HashMap<String, String>,
    pub authentication: HashMap<String, String>,
}

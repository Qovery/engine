use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Deserialize, Serialize)]
pub struct Vpcs {
    pub vpcs: Vec<Vpc>,
}

#[derive(Default, Debug, Deserialize, Serialize, Clone)]
pub struct Vpc {
    pub region: String,
    pub ip_range: String,
    pub name: String,
}

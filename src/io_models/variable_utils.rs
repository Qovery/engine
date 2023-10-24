use serde_derive::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash, Debug)]
pub struct VariableInfo {
    pub value: String,
    pub is_secret: bool,
}

pub fn default_environment_vars_with_info() -> BTreeMap<String, VariableInfo> {
    BTreeMap::new()
}

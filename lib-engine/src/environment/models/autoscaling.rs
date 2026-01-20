use serde_derive::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize, Clone, Eq, PartialEq, Hash, Debug)]
pub struct KedaAuthenticationRef {
    pub name: String,
}

#[derive(Serialize, Clone, Eq, PartialEq, Hash, Debug)]
pub struct KedaTriggerAuthentication {
    pub name: String,
    pub spec: Option<BTreeMap<String, serde_json::Value>>,
    pub raw_yaml: Option<String>,
}

#[derive(Serialize, Clone, Eq, PartialEq, Hash, Debug)]
pub struct KedaScaler {
    pub scaler_type: String,
    pub metadata: Option<BTreeMap<String, String>>,
    pub raw_yaml: Option<String>,
    pub authentication_ref: Option<KedaAuthenticationRef>,
}

#[derive(Serialize, Clone, Eq, PartialEq, Hash, Debug)]
pub struct KedaFallback {
    pub failure_threshold: i32,
    pub replicas: i32,
    pub behavior: Option<String>,
}

#[derive(Serialize, Clone, Eq, PartialEq, Hash, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AutoscalingConfig {
    Keda {
        polling_interval_seconds: Option<u32>,
        cooldown_period_seconds: Option<u32>,
        scalers: Vec<KedaScaler>,
        trigger_authentications: Vec<KedaTriggerAuthentication>,
        fallback: Option<KedaFallback>,
    },
}

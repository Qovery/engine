use serde_derive::Serialize;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ProbeType {
    Exec { commands: Vec<String> },
    Http { path: String, scheme: String },
    Tcp { host: Option<String> },
    Grpc { service: Option<String> },
}

#[derive(Serialize, Clone, Debug)]
pub struct Probe {
    pub r#type: ProbeType,
    pub port: u32,
    pub initial_delay_seconds: u32,
    pub period_seconds: u32,
    pub timeout_seconds: u32,
    pub success_threshold: u32,
    pub failure_threshold: u32,
}

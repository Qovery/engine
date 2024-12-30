use crate::environment::models::probe as models;
use serde_derive::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ProbeType {
    Exec { commands: Vec<String> },
    Http { path: String, scheme: String },
    Tcp { host: Option<String> },
    Grpc { service: Option<String> },
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct Probe {
    pub r#type: ProbeType,
    pub port: u32,
    pub initial_delay_seconds: u32,
    pub period_seconds: u32,
    pub timeout_seconds: u32,
    pub success_threshold: u32,
    pub failure_threshold: u32,
}

impl ProbeType {
    fn to_domain(&self) -> models::ProbeType {
        match self {
            ProbeType::Exec { commands } => models::ProbeType::Exec {
                commands: commands.clone(),
            },
            ProbeType::Http { path, scheme } => models::ProbeType::Http {
                path: path.clone(),
                scheme: scheme.clone(),
            },
            ProbeType::Tcp { host } => models::ProbeType::Tcp { host: host.clone() },
            ProbeType::Grpc { service } => models::ProbeType::Grpc {
                service: service.clone(),
            },
        }
    }
}

impl Probe {
    pub fn to_domain(&self) -> models::Probe {
        models::Probe {
            r#type: self.r#type.to_domain(),
            port: self.port,
            initial_delay_seconds: self.initial_delay_seconds,
            period_seconds: self.period_seconds,
            timeout_seconds: self.timeout_seconds,
            success_threshold: self.success_threshold,
            failure_threshold: self.failure_threshold,
        }
    }
}

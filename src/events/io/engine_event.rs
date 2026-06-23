use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    errors::io::EngineError,
    events::{self, io},
};

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
#[serde(rename_all = "lowercase")]
pub enum EngineEvent {
    Debug {
        r#type: String,
        timestamp: DateTime<Utc>,
        details: EventDetails,
        message: EventMessage,
    },
    Info {
        r#type: String,
        timestamp: DateTime<Utc>,
        details: EventDetails,
        message: EventMessage,
    },
    Warning {
        r#type: String,
        timestamp: DateTime<Utc>,
        details: EventDetails,
        message: EventMessage,
    },
    Error {
        r#type: String,
        timestamp: DateTime<Utc>,
        details: EventDetails,
        error: EngineError,
        message: Option<EventMessage>,
    },
}

impl EngineEvent {
    pub fn timestamp(&self) -> &DateTime<Utc> {
        match self {
            EngineEvent::Debug { timestamp, .. } => timestamp,
            EngineEvent::Info { timestamp, .. } => timestamp,
            EngineEvent::Warning { timestamp, .. } => timestamp,
            EngineEvent::Error { timestamp, .. } => timestamp,
        }
    }
}

impl From<events::EngineEvent> for EngineEvent {
    fn from(event: events::EngineEvent) -> Self {
        let timestamp = Utc::now();
        match event {
            events::EngineEvent::Debug(d, m) => EngineEvent::Debug {
                r#type: "debug".to_string(),
                timestamp,
                details: EventDetails::from(d),
                message: EventMessage::from(m),
            },
            events::EngineEvent::Info(d, m) => EngineEvent::Info {
                r#type: "info".to_string(),
                timestamp,
                details: EventDetails::from(d),
                message: EventMessage::from(m),
            },
            events::EngineEvent::Warning(d, m) => EngineEvent::Warning {
                r#type: "warning".to_string(),
                timestamp,
                details: EventDetails::from(d),
                message: EventMessage::from(m),
            },
            events::EngineEvent::Error(e, m) => {
                let (engine_error, details) = EngineError::from(e);
                EngineEvent::Error {
                    r#type: "error".to_string(),
                    timestamp,
                    details: EventDetails::from(details),
                    error: engine_error,
                    message: m.map(EventMessage::from),
                }
            }
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub struct EventMessage {
    safe_message: String,
    full_details: Option<String>,
}

impl From<events::EventMessage> for EventMessage {
    fn from(message: events::EventMessage) -> Self {
        EventMessage {
            safe_message: message.safe_message,
            full_details: message.full_details,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
#[serde(tag = "type", content = "step")]
pub enum Stage {
    Infrastructure(io::InfrastructureStep),
    Environment(io::EnvironmentStep),
    Blueprint(io::BlueprintStep),
    #[serde(rename = "cluster-analysis")]
    ClusterAnalysis(io::ClusterAnalysisStep),
}

impl From<events::Stage> for Stage {
    fn from(stage: events::Stage) -> Self {
        match stage {
            events::Stage::Infrastructure(step) => Stage::Infrastructure(step.into()),
            events::Stage::Environment(step) => Stage::Environment(step.into()),
            events::Stage::Blueprint(step) => Stage::Blueprint(step.into()),
            events::Stage::ClusterAnalysis(step) => Stage::ClusterAnalysis(step.into()),
        }
    }
}

type TransmitterId = Uuid;
type TransmitterName = String;

#[derive(Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum Transmitter {
    Blueprint { id: TransmitterId, name: TransmitterName },
    TaskManager { id: TransmitterId, name: TransmitterName },
    BuildPlatform { id: TransmitterId, name: TransmitterName },
    ContainerRegistry { id: TransmitterId, name: TransmitterName },
    CloudProvider { id: TransmitterId, name: TransmitterName },
    Kubernetes { id: TransmitterId, name: TransmitterName },
    DnsProvider { id: TransmitterId, name: TransmitterName },
    ObjectStorage { id: TransmitterId, name: TransmitterName },
    Environment { id: TransmitterId, name: TransmitterName },
    Database { id: TransmitterId, name: TransmitterName },
    Application { id: TransmitterId, name: TransmitterName },
    Container { id: TransmitterId, name: TransmitterName },
    Router { id: TransmitterId, name: TransmitterName },
    Job { id: TransmitterId, name: TransmitterName },
    Helm { id: TransmitterId, name: TransmitterName },
    Terraform { id: TransmitterId, name: TransmitterName },
}

impl From<events::Transmitter> for Transmitter {
    fn from(transmitter: events::Transmitter) -> Self {
        match transmitter {
            events::Transmitter::TaskManager(id, name) => Transmitter::TaskManager { id, name },
            events::Transmitter::BuildPlatform(id, name) => Transmitter::BuildPlatform { id, name },
            events::Transmitter::ContainerRegistry(id, name) => Transmitter::ContainerRegistry { id, name },
            events::Transmitter::CloudProvider(id, name) => Transmitter::CloudProvider { id, name },
            events::Transmitter::Kubernetes(id, name) => Transmitter::Kubernetes { id, name },
            events::Transmitter::DnsProvider(id, name) => Transmitter::DnsProvider { id, name },
            events::Transmitter::ObjectStorage(id, name) => Transmitter::ObjectStorage { id, name },
            events::Transmitter::Environment(id, name) => Transmitter::Environment { id, name },
            events::Transmitter::Database(id, name) => Transmitter::Database { id, name },
            events::Transmitter::Application(id, name) => Transmitter::Application { id, name },
            events::Transmitter::Router(id, name) => Transmitter::Router { id, name },
            events::Transmitter::Container(id, name) => Transmitter::Container { id, name },
            events::Transmitter::Job(id, name) => Transmitter::Job { id, name },
            events::Transmitter::Helm(id, name) => Transmitter::Helm { id, name },
            events::Transmitter::TerraformService(id, name) => Transmitter::Terraform { id, name },
            events::Transmitter::Blueprint(id, name) => Transmitter::Blueprint { id, name },
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub struct EventDetails {
    organization_id: String,
    cluster_id: String,
    execution_id: String,
    stage: Stage,
    transmitter: Transmitter,
}

impl From<events::EventDetails> for EventDetails {
    fn from(details: events::EventDetails) -> Self {
        EventDetails {
            organization_id: details.organisation_id.to_string(),
            cluster_id: details.cluster_id.to_string(),
            execution_id: details.execution_id.to_string(),
            stage: Stage::from(details.stage),
            transmitter: Transmitter::from(details.transmitter),
        }
    }
}
#[cfg(test)]
mod test {
    use crate::errors::EngineError;
    use crate::events::io;
    use crate::events::{EngineEvent, EventDetails, InfrastructureStep, Stage, Transmitter};
    use crate::infrastructure::models::cloud_provider::Kind;
    use crate::io_models::QoveryIdentifier;
    use uuid::Uuid;

    #[test]
    fn should_use_default_enum_value_when_serializing_infrastructure_step() {
        // setup:
        let engine_err = EngineError::new_unknown(
            EventDetails::new(
                Some(Kind::Scw),
                QoveryIdentifier::new_random(),
                QoveryIdentifier::new_random(),
                QoveryIdentifier::new_random().to_string(),
                Stage::Infrastructure(InfrastructureStep::CreateError),
                Transmitter::Kubernetes(Uuid::new_v4(), "".to_string()),
            ),
            "user_log_message".to_string(),
            None,
            None,
            None,
        );
        let event = EngineEvent::Error(engine_err, None);
        let event_io = io::EngineEvent::from(event);

        // compute:
        match serde_json::to_string(&event_io) {
            Ok(json) => {
                // validate:
                assert!(json.contains(r#"{"type":"infrastructure","step":"CreateError"}"#))
            }
            Err(e) => {
                panic!("Panic ! Error: {e}")
            }
        }
    }
}

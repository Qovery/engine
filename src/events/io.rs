use crate::cloud_provider::io::Kind;
use crate::errors::io::EngineError;
use crate::events;
use serde_derive::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", content = "event")]
#[serde(rename_all = "lowercase")]
pub enum EngineEvent {
    Error(EngineError),
    Waiting(EventDetails, EventMessage),
    Deploying(EventDetails, EventMessage),
    Pausing(EventDetails, EventMessage),
    Deleting(EventDetails, EventMessage),
    Deployed(EventDetails, EventMessage),
    Paused(EventDetails, EventMessage),
    Deleted(EventDetails, EventMessage),
}

impl From<events::EngineEvent> for EngineEvent {
    fn from(event: events::EngineEvent) -> Self {
        match event {
            events::EngineEvent::Error(e) => EngineEvent::Error(EngineError::from(e)),
            events::EngineEvent::Waiting(d, m) => EngineEvent::Waiting(EventDetails::from(d), EventMessage::from(m)),
            events::EngineEvent::Deploying(d, m) => {
                EngineEvent::Deploying(EventDetails::from(d), EventMessage::from(m))
            }
            events::EngineEvent::Pausing(d, m) => EngineEvent::Pausing(EventDetails::from(d), EventMessage::from(m)),
            events::EngineEvent::Deleting(d, m) => EngineEvent::Deleting(EventDetails::from(d), EventMessage::from(m)),
            events::EngineEvent::Deployed(d, m) => EngineEvent::Deployed(EventDetails::from(d), EventMessage::from(m)),
            events::EngineEvent::Paused(d, m) => EngineEvent::Paused(EventDetails::from(d), EventMessage::from(m)),
            events::EngineEvent::Deleted(d, m) => EngineEvent::Deleted(EventDetails::from(d), EventMessage::from(m)),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub struct EventMessage {
    raw: String,
    safe: Option<String>,
}

impl From<events::EventMessage> for EventMessage {
    fn from(message: events::EventMessage) -> Self {
        EventMessage {
            raw: message.raw,
            safe: message.safe,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    Infrastructure(InfrastructureStep),
    Environment(EnvironmentStep),
}

impl From<events::Stage> for Stage {
    fn from(stage: events::Stage) -> Self {
        match stage {
            events::Stage::Infrastructure(step) => Stage::Infrastructure(InfrastructureStep::from(step)),
            events::Stage::Environment(step) => Stage::Environment(EnvironmentStep::from(step)),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InfrastructureStep {
    Instantiate,
    Create,
    Pause,
    Upgrade,
    Delete,
}

impl From<events::InfrastructureStep> for InfrastructureStep {
    fn from(step: events::InfrastructureStep) -> Self {
        match step {
            events::InfrastructureStep::LoadConfiguration => InfrastructureStep::Instantiate,
            events::InfrastructureStep::Create => InfrastructureStep::Create,
            events::InfrastructureStep::Pause => InfrastructureStep::Pause,
            events::InfrastructureStep::Upgrade => InfrastructureStep::Upgrade,
            events::InfrastructureStep::Delete => InfrastructureStep::Delete,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EnvironmentStep {
    Build,
    Deploy,
    Update,
    Delete,
}

impl From<events::EnvironmentStep> for EnvironmentStep {
    fn from(step: events::EnvironmentStep) -> Self {
        match step {
            events::EnvironmentStep::Build => EnvironmentStep::Build,
            events::EnvironmentStep::Deploy => EnvironmentStep::Deploy,
            events::EnvironmentStep::Update => EnvironmentStep::Update,
            events::EnvironmentStep::Delete => EnvironmentStep::Delete,
        }
    }
}

type TransmitterId = String;
type TransmitterName = String;
type TransmitterType = String;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Transmitter {
    Engine,
    BuildPlatform(TransmitterId, TransmitterName),
    ContainerRegistry(TransmitterId, TransmitterName),
    CloudProvider(TransmitterId, TransmitterName),
    Kubernetes(TransmitterId, TransmitterName),
    DnsProvider(TransmitterId, TransmitterName),
    ObjectStorage(TransmitterId, TransmitterName),
    Environment(TransmitterId, TransmitterName),
    Database(TransmitterId, TransmitterType, TransmitterName),
    Application(TransmitterId, TransmitterName),
    Router(TransmitterId, TransmitterName),
}

impl From<events::Transmitter> for Transmitter {
    fn from(transmitter: events::Transmitter) -> Self {
        match transmitter {
            events::Transmitter::Engine => Transmitter::Engine,
            events::Transmitter::BuildPlatform(id, name) => Transmitter::BuildPlatform(id, name),
            events::Transmitter::ContainerRegistry(id, name) => Transmitter::ContainerRegistry(id, name),
            events::Transmitter::CloudProvider(id, name) => Transmitter::CloudProvider(id, name),
            events::Transmitter::Kubernetes(id, name) => Transmitter::Kubernetes(id, name),
            events::Transmitter::DnsProvider(id, name) => Transmitter::DnsProvider(id, name),
            events::Transmitter::ObjectStorage(id, name) => Transmitter::ObjectStorage(id, name),
            events::Transmitter::Environment(id, name) => Transmitter::Environment(id, name),
            events::Transmitter::Database(id, db_type, name) => Transmitter::Database(id, db_type, name),
            events::Transmitter::Application(id, name) => Transmitter::Application(id, name),
            events::Transmitter::Router(id, name) => Transmitter::Router(id, name),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tag {
    UnsupportedInstanceType(String),
}

impl From<events::Tag> for Tag {
    fn from(tag: events::Tag) -> Self {
        match tag {
            events::Tag::UnsupportedInstanceType(s) => Tag::UnsupportedInstanceType(s),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub struct EventDetails {
    provider_kind: Kind,
    organisation_id: String,
    cluster_id: String,
    execution_id: String,
    region: String,
    stage: Stage,
    transmitter: Transmitter,
}

impl From<events::EventDetails> for EventDetails {
    fn from(details: events::EventDetails) -> Self {
        EventDetails {
            provider_kind: Kind::from(details.provider_kind),
            organisation_id: details.organisation_id.to_string(),
            cluster_id: details.cluster_id.to_string(),
            execution_id: details.execution_id.to_string(),
            region: details.region,
            stage: Stage::from(details.stage),
            transmitter: Transmitter::from(details.transmitter),
        }
    }
}

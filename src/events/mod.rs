pub mod io;

extern crate url;

use crate::cloud_provider::Kind;
use crate::errors::EngineError;
use crate::models::QoveryIdentifier;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone)]
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

impl EngineEvent {
    pub fn get_details(&self) -> &EventDetails {
        match self {
            EngineEvent::Error(engine_error) => engine_error.event_details(),
            EngineEvent::Waiting(details, _message) => details,
            EngineEvent::Deploying(details, _message) => details,
            EngineEvent::Pausing(details, _message) => details,
            EngineEvent::Deleting(details, _message) => details,
            EngineEvent::Deployed(details, _message) => details,
            EngineEvent::Paused(details, _message) => details,
            EngineEvent::Deleted(details, _message) => details,
        }
    }

    pub fn get_message(&self) -> String {
        match self {
            EngineEvent::Error(engine_error) => engine_error.message(),
            EngineEvent::Waiting(_details, message) => message.get_message(),
            EngineEvent::Deploying(_details, message) => message.get_message(),
            EngineEvent::Pausing(_details, message) => message.get_message(),
            EngineEvent::Deleting(_details, message) => message.get_message(),
            EngineEvent::Deployed(_details, message) => message.get_message(),
            EngineEvent::Paused(_details, message) => message.get_message(),
            EngineEvent::Deleted(_details, message) => message.get_message(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EventMessage {
    raw: String,
    safe: Option<String>,
}

impl EventMessage {
    pub fn new(raw: String, safe: Option<String>) -> Self {
        EventMessage { raw, safe }
    }

    /// Returns message for event message, safe message if exists, otherwise raw.
    pub fn get_message(&self) -> String {
        if let Some(msg) = &self.safe {
            return msg.clone();
        }

        self.raw.clone()
    }
}

impl Display for EventMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match &self.safe {
            Some(safe) => safe,
            None => &self.raw,
        })
    }
}

#[derive(Debug, Clone)]
pub enum Stage {
    Infrastructure(InfrastructureStep),
    Environment(EnvironmentStep),
}

impl Stage {
    pub fn sub_step_name(&self) -> String {
        match &self {
            Stage::Infrastructure(step) => step.to_string(),
            Stage::Environment(step) => step.to_string(),
        }
    }
}

impl Display for Stage {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match &self {
                Stage::Infrastructure(_) => "infrastructure",
                Stage::Environment(_) => "environment",
            },
        )
    }
}

#[derive(Debug, Clone)]
pub enum InfrastructureStep {
    Instantiate,
    Create,
    Pause,
    Upgrade,
    Delete,
}

impl Display for InfrastructureStep {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match &self {
                InfrastructureStep::Instantiate => "instanciate",
                InfrastructureStep::Create => "create",
                InfrastructureStep::Pause => "pause",
                InfrastructureStep::Upgrade => "upgrade",
                InfrastructureStep::Delete => "delete",
            },
        )
    }
}

#[derive(Debug, Clone)]
pub enum EnvironmentStep {
    Build,
    Deploy,
    Update,
    Delete,
}

impl Display for EnvironmentStep {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match &self {
                EnvironmentStep::Build => "build",
                EnvironmentStep::Deploy => "deploy",
                EnvironmentStep::Update => "update",
                EnvironmentStep::Delete => "delete",
            },
        )
    }
}

type TransmitterId = String;
type TransmitterName = String;
type TransmitterType = String;

#[derive(Debug, Clone)]
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

impl Display for Transmitter {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match &self {
                Transmitter::Engine => "engine".to_string(),
                Transmitter::BuildPlatform(id, name) => format!("build_platform({}, {})", id, name),
                Transmitter::ContainerRegistry(id, name) => format!("container_registry({}, {})", id, name),
                Transmitter::CloudProvider(id, name) => format!("cloud_provider({}, {})", id, name),
                Transmitter::Kubernetes(id, name) => format!("kubernetes({}, {})", id, name),
                Transmitter::DnsProvider(id, name) => format!("dns_provider({}, {})", id, name),
                Transmitter::ObjectStorage(id, name) => format!("object_strorage({}, {})", id, name),
                Transmitter::Environment(id, name) => format!("environment({}, {})", id, name),
                Transmitter::Database(id, db_type, name) => format!("database({}, {}, {})", id, db_type, name),
                Transmitter::Application(id, name) => format!("application({}, {})", id, name),
                Transmitter::Router(id, name) => format!("router({}, {})", id, name),
            }
        )
    }
}

#[derive(Debug, Clone)]
pub enum Tag {
    UnsupportedInstanceType(String),
}

type Region = String;

#[derive(Debug, Clone)]
pub struct EventDetails {
    provider_kind: Kind,
    organisation_id: QoveryIdentifier,
    cluster_id: QoveryIdentifier,
    execution_id: QoveryIdentifier,
    region: Region, // TODO(benjaminch): find a way to make Region a real struct type
    stage: Stage,
    transmitter: Transmitter,
}

impl EventDetails {
    pub fn new(
        provider_kind: Kind,
        organisation_id: QoveryIdentifier,
        cluster_id: QoveryIdentifier,
        execution_id: QoveryIdentifier,
        region: Region,
        stage: Stage,
        transmitter: Transmitter,
    ) -> Self {
        EventDetails {
            provider_kind,
            organisation_id,
            cluster_id,
            execution_id,
            region,
            stage,
            transmitter,
        }
    }
    pub fn provider_kind(&self) -> &Kind {
        &self.provider_kind
    }
    pub fn organisation_id(&self) -> &QoveryIdentifier {
        &self.organisation_id
    }
    pub fn cluster_id(&self) -> &QoveryIdentifier {
        &self.cluster_id
    }
    pub fn execution_id(&self) -> &QoveryIdentifier {
        &self.execution_id
    }
    pub fn region(&self) -> &Region {
        &self.region
    }
    pub fn stage(&self) -> &Stage {
        &self.stage
    }
    pub fn transmitter(&self) -> Transmitter {
        self.transmitter.clone()
    }
}

pub mod io;

extern crate url;

use crate::cloud_provider::Kind;
use crate::errors::EngineError;
use crate::models::QoveryIdentifier;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone)]
/// EngineEvent: represents an event happening in the Engine.
pub enum EngineEvent {
    /// Error: represents an error event.
    Error(EngineError),
    /// Waiting: represents an engine waiting event.
    ///
    /// Engine is waiting for a task to be done.
    Waiting(EventDetails, EventMessage),
    /// Deploying: represents an engine deploying event.
    Deploying(EventDetails, EventMessage),
    /// Pausing: represents an engine pausing event.
    Pausing(EventDetails, EventMessage),
    /// Deleting: represents an engine deleting event.
    Deleting(EventDetails, EventMessage),
    /// Deployed: represents an engine deployed event.
    Deployed(EventDetails, EventMessage),
    /// Paused: represents an engine paused event.
    Paused(EventDetails, EventMessage),
    /// Deleted: represents an engine deleted event.
    Deleted(EventDetails, EventMessage),
}

impl EngineEvent {
    /// Returns engine's event details.
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

    /// Returns engine's event message.
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
/// EventMessage: represents an event message.
pub struct EventMessage {
    /// raw: represents a raw event message which may include unsafe elements such as passwords and tokens.
    raw: String,
    /// safe: represents an event message from which unsafe elements have been removed (passwords and tokens).
    safe: Option<String>,
}

impl EventMessage {
    /// Creates e new EventMessage.
    ///
    /// Arguments
    ///
    /// * `raw`: Event raw message string (which may include unsafe text such as passwords and tokens).
    /// * `safe`: Event safe message string (from which all unsafe text such as passwords and tokens has been removed).
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
/// Stage: represents an engine event stage, can be Infrastructure or Environment.
pub enum Stage {
    /// Infrastructure: infrastructure stage in the engine (clusters operations).
    Infrastructure(InfrastructureStep),
    /// Environment: environment stage in the engine (applications operations).
    Environment(EnvironmentStep),
}

impl Stage {
    /// Returns stage's sub step name.
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
/// InfrastructureStep: represents an engine infrastructure step.
pub enum InfrastructureStep {
    /// LoadConfiguration: first step in infrastructure, aiming to load all configuration (from Terraform, etc).
    LoadConfiguration,
    /// Create: creating a cluster.
    Create,
    /// Pause: pausing a cluster.
    Pause,
    /// Resume: resume a paused cluster.
    Resume,
    /// Upgrade: upgrade a cluster.
    Upgrade,
    /// Delete: delete a cluster.
    Delete,
}

impl Display for InfrastructureStep {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match &self {
                InfrastructureStep::LoadConfiguration => "load-configuration",
                InfrastructureStep::Create => "create",
                InfrastructureStep::Pause => "pause",
                InfrastructureStep::Upgrade => "upgrade",
                InfrastructureStep::Delete => "delete",
                InfrastructureStep::Resume => "resume",
            },
        )
    }
}

#[derive(Debug, Clone)]
/// EnvironmentStep: represents an engine environment step.
pub enum EnvironmentStep {
    /// Build: building an application (docker or build packs).
    Build,
    /// Deploy: deploy an environment (application to kubernetes).
    Deploy,
    /// Pause: pause an environment.
    Pause,
    /// Resume: resume a paused environment.
    Resume,
    /// Update: update an environment.
    Update,
    /// Delete: delete an environment.
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
                EnvironmentStep::Pause => "pause",
                EnvironmentStep::Resume => "resume",
            },
        )
    }
}

/// TransmitterId: represents a transmitter unique identifier.
type TransmitterId = String;
/// TransmitterName: represents a transmitter name.
type TransmitterName = String;
/// TransmitterType: represents a transmitter type.
type TransmitterType = String;

#[derive(Debug, Clone)]
/// Transmitter: represents the event's source caller (transmitter).
pub enum Transmitter {
    /// BuildPlatform: platform aiming to build applications images.
    BuildPlatform(TransmitterId, TransmitterName),
    /// ContainerRegistry: container registry engine part.
    ContainerRegistry(TransmitterId, TransmitterName),
    /// CloudProvider: cloud provider engine part.
    CloudProvider(TransmitterId, TransmitterName),
    /// Kubernetes: kubernetes infrastructure engine part.
    Kubernetes(TransmitterId, TransmitterName),
    /// DnsProvider: DNS provider engine part.
    DnsProvider(TransmitterId, TransmitterName),
    /// ObjectStorage: object storage engine part.
    ObjectStorage(TransmitterId, TransmitterName),
    /// Environment: environment engine part.
    Environment(TransmitterId, TransmitterName),
    /// Database: database engine part.
    Database(TransmitterId, TransmitterType, TransmitterName),
    /// Application: application engine part.
    Application(TransmitterId, TransmitterName),
    /// Router: router engine part.
    Router(TransmitterId, TransmitterName),
}

impl Display for Transmitter {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match &self {
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

/// Region: represents event's cloud provider region.
type Region = String;

#[derive(Debug, Clone)]
/// EventDetails: represents an event details, carrying all useful data such as Qovery identifiers, transmitter, stage etc.
pub struct EventDetails {
    /// provider_kind: cloud provider name
    provider_kind: Kind,
    /// organisation_id: Qovery organisation identifier.
    organisation_id: QoveryIdentifier,
    /// cluster_id: Qovery cluster identifier.
    cluster_id: QoveryIdentifier,
    /// execution_id: Qovery execution identifier.
    execution_id: QoveryIdentifier,
    /// region: event's region (cloud provider specific region).
    region: Region, // TODO(benjaminch): find a way to make Region a real struct type
    /// stage: stage in which this event has been triggered.
    stage: Stage,
    /// transmitter: source triggering the event.
    transmitter: Transmitter,
}

impl EventDetails {
    /// Creates a new EventDetails.
    ///
    /// Arguments
    ///
    /// * `provider_kind`: Cloud provider name.
    /// * `organisation_id`: Qovery's organisation identifier.
    /// * `cluster_id`: Qovery's cluster identifier.
    /// * `execution_id`: Qovery's execution identifier.
    /// * `region`: Event's region (cloud provider region).
    /// * `stage`: Event's stage in which this event has been triggered.
    /// * `transmitter`: Event's source transmitter.
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

    /// Returns event's provider name.
    pub fn provider_kind(&self) -> &Kind {
        &self.provider_kind
    }

    /// Returns event's Qovery organisation identifier.
    pub fn organisation_id(&self) -> &QoveryIdentifier {
        &self.organisation_id
    }

    /// Returns event's Qovery cluster identifier.
    pub fn cluster_id(&self) -> &QoveryIdentifier {
        &self.cluster_id
    }

    /// Returns event's Qovery execution identifier.
    pub fn execution_id(&self) -> &QoveryIdentifier {
        &self.execution_id
    }

    /// Returns event's region (cloud provider region).
    pub fn region(&self) -> &Region {
        &self.region
    }

    /// Returns event's stage in which the event has been triggered.
    pub fn stage(&self) -> &Stage {
        &self.stage
    }

    /// Returns event's source transmitter.
    pub fn transmitter(&self) -> Transmitter {
        self.transmitter.clone()
    }
}

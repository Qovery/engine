#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::large_enum_variant)]
#![allow(deprecated)]

pub mod io;

extern crate url;

use crate::cloud_provider::Kind;
use crate::errors::{CommandError, EngineError, ErrorMessageVerbosity};
use crate::io_models::QoveryIdentifier;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone)]
/// EngineEvent: represents an event happening in the Engine.
pub enum EngineEvent {
    /// Debug: represents a debug message event.
    Debug(EventDetails, EventMessage),
    /// Info: represents an info message event.
    Info(EventDetails, EventMessage),
    /// Warning: represents a warning message event.
    Warning(EventDetails, EventMessage),
    /// Error: represents an error event.
    Error(EngineError, Option<EventMessage>),
}

impl EngineEvent {
    /// Returns engine's event details.
    pub fn get_details(&self) -> &EventDetails {
        match self {
            EngineEvent::Debug(details, _message) => details,
            EngineEvent::Info(details, _message) => details,
            EngineEvent::Warning(details, _message) => details,
            EngineEvent::Error(engine_error, _message) => engine_error.event_details(),
        }
    }

    /// Returns engine's event message.
    pub fn message(&self, message_verbosity: EventMessageVerbosity) -> String {
        match self {
            EngineEvent::Debug(_details, message) => message.message(message_verbosity),
            EngineEvent::Info(_details, message) => message.message(message_verbosity),
            EngineEvent::Warning(_details, message) => message.message(message_verbosity),
            EngineEvent::Error(engine_error, _message) => engine_error.message(message_verbosity.into()),
        }
    }
}

/// EventMessageVerbosity: represents event message's verbosity from minimal to full verbosity.
pub enum EventMessageVerbosity {
    SafeOnly,
    FullDetailsWithoutEnvVars,
    FullDetails,
}

impl From<EventMessageVerbosity> for ErrorMessageVerbosity {
    fn from(verbosity: EventMessageVerbosity) -> Self {
        match verbosity {
            EventMessageVerbosity::SafeOnly => ErrorMessageVerbosity::SafeOnly,
            EventMessageVerbosity::FullDetailsWithoutEnvVars => ErrorMessageVerbosity::FullDetailsWithoutEnvVars,
            EventMessageVerbosity::FullDetails => ErrorMessageVerbosity::FullDetails,
        }
    }
}

#[derive(Debug, Clone)]
/// EventMessage: represents an event message.
pub struct EventMessage {
    // Message which is known to be safe: doesn't expose any credentials nor touchy info.
    safe_message: String,
    // String containing full details including touchy data (passwords and tokens).
    full_details: Option<String>,
    // Environments variables including touchy data such as secret keys.
    env_vars: Option<Vec<(String, String)>>,
}

impl EventMessage {
    /// Creates e new EventMessage.
    ///
    /// Arguments
    ///
    /// * `safe_message`: Event safe message string (from which all unsafe text such as passwords and tokens has been removed).
    /// * `full_details`: Event raw message string (which may include unsafe text such as passwords and tokens).
    pub fn new(safe_message: String, full_details: Option<String>) -> Self {
        EventMessage {
            safe_message,
            full_details,
            env_vars: None,
        }
    }

    /// Creates e new EventMessage with environment variables.
    ///
    /// Arguments
    ///
    /// * `safe_message`: Event safe message string (from which all unsafe text such as passwords and tokens has been removed).
    /// * `full_details`: Event raw message string (which may include unsafe text such as passwords and tokens).
    /// * `env_vars`: Event environment variables (which may contains unsafe text such as secrets keys).
    pub fn new_with_env_vars(
        safe_message: String,
        full_details: Option<String>,
        env_vars: Option<Vec<(String, String)>>,
    ) -> Self {
        EventMessage {
            safe_message,
            full_details,
            env_vars,
        }
    }

    /// Creates e new EventMessage from safe message.
    ///
    /// Arguments
    ///
    /// * `safe_message`: Event safe message string (from which all unsafe text such as passwords and tokens has been removed).
    pub fn new_from_safe(safe_message: String) -> Self {
        EventMessage {
            safe_message,
            full_details: None,
            env_vars: None,
        }
    }

    /// Returns message for event message.
    ///
    /// Arguments
    ///
    /// * `message_verbosity`: Which verbosity is required for the message.
    pub fn message(&self, message_verbosity: EventMessageVerbosity) -> String {
        match message_verbosity {
            EventMessageVerbosity::SafeOnly => self.safe_message.to_string(),
            EventMessageVerbosity::FullDetailsWithoutEnvVars => match &self.full_details {
                None => self.safe_message.to_string(),
                Some(details) => format!("{} / Full details: {}", self.safe_message, details),
            },
            EventMessageVerbosity::FullDetails => match &self.full_details {
                None => self.safe_message.to_string(),
                Some(details) => match &self.env_vars {
                    None => format!("{} / Full details: {}", self.safe_message, details),
                    Some(env_vars) => {
                        format!(
                            "{} / Full details: {} / Env vars: {}",
                            self.safe_message,
                            details,
                            env_vars
                                .iter()
                                .map(|(k, v)| format!("{}={}", k, v))
                                .collect::<Vec<String>>()
                                .join(" "),
                        )
                    }
                },
            },
        }
    }
}

impl From<CommandError> for EventMessage {
    fn from(e: CommandError) -> Self {
        EventMessage::new_with_env_vars(e.message_raw(), e.message_safe(), e.env_vars())
    }
}

impl Display for EventMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message(EventMessageVerbosity::SafeOnly).as_str()) // By default, expose only the safe message.
    }
}

#[derive(Debug, Clone, PartialEq)]
/// Stage: represents an engine event stage, can be General, Infrastructure or Environment.
pub enum Stage {
    /// GeneralStep: general stage in the engine, usually used across all stages.
    General(GeneralStep),
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
            Stage::General(step) => step.to_string(),
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
                Stage::General(_) => "general",
            },
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
/// GeneralStep: represents an engine general step usually shared across all engine stages
pub enum GeneralStep {
    /// ValidateSystemRequirements: validating system requirements
    ValidateSystemRequirements,
    /// RetrieveClusterConfig: retrieving cluster configuration
    RetrieveClusterConfig,
    /// RetrieveClusterResources: retrieving cluster resources
    RetrieveClusterResources,
    /// UnderMigration: error migration hasn't been completed yet.
    UnderMigration,
}

impl Display for GeneralStep {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match &self {
                GeneralStep::RetrieveClusterConfig => "retrieve-cluster-config",
                GeneralStep::RetrieveClusterResources => "retrieve-cluster-resources",
                GeneralStep::ValidateSystemRequirements => "validate-system-requirements",
                GeneralStep::UnderMigration => "under-migration",
            }
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
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
    /// Downgrade: upgrade a cluster.
    Downgrade,
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
                InfrastructureStep::Downgrade => "downgrade",
                InfrastructureStep::Delete => "delete",
                InfrastructureStep::Resume => "resume",
            },
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
/// EnvironmentStep: represents an engine environment step.
pub enum EnvironmentStep {
    /// LoadConfiguration: first step in environment, aiming to load all configuration (from Terraform, etc).
    LoadConfiguration,
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
    /// ScaleDown: scale up an environment.
    ScaleUp,
    /// ScaleDown: scale down an environment.
    ScaleDown,
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
                EnvironmentStep::LoadConfiguration => "load-configuration",
                EnvironmentStep::ScaleUp => "scale-up",
                EnvironmentStep::ScaleDown => "scale-down",
            },
        )
    }
}

pub trait ToTransmitter {
    fn to_transmitter(&self) -> Transmitter;
}

/// TransmitterId: represents a transmitter unique identifier.
type TransmitterId = String;
/// TransmitterName: represents a transmitter name.
type TransmitterName = String;
/// TransmitterType: represents a transmitter type.
type TransmitterType = String; // TODO(benjaminch): makes it a real enum / type

#[derive(Debug, Clone, PartialEq)]
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

#[derive(Debug, Clone, PartialEq)]
/// EventDetails: represents an event details, carrying all useful data such as Qovery identifiers, transmitter, stage etc.
pub struct EventDetails {
    /// provider_kind: cloud provider name. an be set to None if not linked to any provider kind.
    provider_kind: Option<Kind>,
    /// organisation_id: Qovery organisation identifier.
    organisation_id: QoveryIdentifier,
    /// cluster_id: Qovery cluster identifier.
    cluster_id: QoveryIdentifier,
    /// execution_id: Qovery execution identifier.
    execution_id: QoveryIdentifier,
    /// region: event's region (cloud provider specific region). Can be set to None if not applicable in the case of an application for example.
    region: Option<Region>, // TODO(benjaminch): find a way to make Region a real struct type
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
        provider_kind: Option<Kind>,
        organisation_id: QoveryIdentifier,
        cluster_id: QoveryIdentifier,
        execution_id: QoveryIdentifier,
        region: Option<Region>,
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

    /// TODO(benjaminch): remove this dirty hack
    pub fn clone_changing_stage(event_details: EventDetails, stage: Stage) -> Self {
        let mut event_details = event_details;
        event_details.stage = stage;
        event_details
    }

    /// Returns event's provider name.
    pub fn provider_kind(&self) -> Option<Kind> {
        self.provider_kind.clone()
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
    pub fn region(&self) -> Option<Region> {
        self.region.clone()
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

#[cfg(test)]
mod tests {
    use crate::events::{EnvironmentStep, EventMessage, EventMessageVerbosity, InfrastructureStep, Stage};

    #[test]
    fn test_event_message() {
        // setup:
        let test_cases: Vec<(
            String,
            Option<String>,
            Option<Vec<(String, String)>>,
            EventMessageVerbosity,
            String,
        )> = vec![
            (
                "safe".to_string(),
                Some("raw".to_string()),
                Some(vec![("env".to_string(), "value".to_string())]),
                EventMessageVerbosity::SafeOnly,
                "safe".to_string(),
            ),
            (
                "safe".to_string(),
                None,
                None,
                EventMessageVerbosity::SafeOnly,
                "safe".to_string(),
            ),
            (
                "safe".to_string(),
                None,
                None,
                EventMessageVerbosity::FullDetails,
                "safe".to_string(),
            ),
            (
                "safe".to_string(),
                Some("raw".to_string()),
                None,
                EventMessageVerbosity::FullDetails,
                "safe / Full details: raw".to_string(),
            ),
            (
                "safe".to_string(),
                Some("raw".to_string()),
                Some(vec![("env".to_string(), "value".to_string())]),
                EventMessageVerbosity::FullDetailsWithoutEnvVars,
                "safe / Full details: raw".to_string(),
            ),
            (
                "safe".to_string(),
                Some("raw".to_string()),
                Some(vec![("env".to_string(), "value".to_string())]),
                EventMessageVerbosity::FullDetails,
                "safe / Full details: raw / Env vars: env=value".to_string(),
            ),
        ];

        for tc in test_cases {
            // execute:
            let (safe_message, raw_message, env_vars, verbosity, expected) = tc;
            let event_message = EventMessage::new_with_env_vars(safe_message, raw_message, env_vars);

            // validate:
            assert_eq!(expected, event_message.message(verbosity));
        }
    }

    #[test]
    fn test_stage_sub_step_name() {
        // setup:
        let test_cases: Vec<(Stage, String)> = vec![
            (
                Stage::Infrastructure(InfrastructureStep::Create),
                InfrastructureStep::Create.to_string(),
            ),
            (
                Stage::Infrastructure(InfrastructureStep::Upgrade),
                InfrastructureStep::Upgrade.to_string(),
            ),
            (
                Stage::Infrastructure(InfrastructureStep::Delete),
                InfrastructureStep::Delete.to_string(),
            ),
            (
                Stage::Infrastructure(InfrastructureStep::Resume),
                InfrastructureStep::Resume.to_string(),
            ),
            (
                Stage::Infrastructure(InfrastructureStep::Pause),
                InfrastructureStep::Pause.to_string(),
            ),
            (
                Stage::Infrastructure(InfrastructureStep::LoadConfiguration),
                InfrastructureStep::LoadConfiguration.to_string(),
            ),
            (Stage::Environment(EnvironmentStep::Pause), EnvironmentStep::Pause.to_string()),
            (Stage::Environment(EnvironmentStep::Resume), EnvironmentStep::Resume.to_string()),
            (Stage::Environment(EnvironmentStep::Build), EnvironmentStep::Build.to_string()),
            (Stage::Environment(EnvironmentStep::Delete), EnvironmentStep::Delete.to_string()),
            (Stage::Environment(EnvironmentStep::Update), EnvironmentStep::Update.to_string()),
            (Stage::Environment(EnvironmentStep::Deploy), EnvironmentStep::Deploy.to_string()),
        ];

        for tc in test_cases {
            // execute:
            let (stage, expected_step_name) = tc;
            let result = stage.sub_step_name();

            // validate:
            assert_eq!(expected_step_name, result);
        }
    }
}

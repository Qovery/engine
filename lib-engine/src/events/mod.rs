#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::large_enum_variant)]
#![allow(deprecated)]

pub mod io;

extern crate derivative;
extern crate url;

use crate::cloud_provider::Kind;
use crate::errors::{CommandError, EngineError, ErrorMessageVerbosity};
use crate::io_models::QoveryIdentifier;
use derivative::Derivative;
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

#[derive(Derivative, Clone)]
#[derivative(Debug)]
/// EventMessage: represents an event message.
pub struct EventMessage {
    // Message which is known to be safe: doesn't expose any credentials nor touchy info.
    safe_message: String,
    // String containing full details including touchy data (passwords and tokens).
    full_details: Option<String>,
    // Environments variables including touchy data such as secret keys.
    // env_vars field is ignored from any wild Debug printing because of it touchy data it carries.
    #[derivative(Debug = "ignore")]
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
        EventMessage::new_with_env_vars(e.message_safe(), e.message_raw(), e.env_vars())
    }
}

impl Display for EventMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message(EventMessageVerbosity::SafeOnly).as_str()) // By default, expose only the safe message.
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
/// GeneralStep: represents an engine general step usually shared across all engine stages
pub enum GeneralStep {
    /// ValidateApiInput: validating Engine's API input
    ValidateApiInput,
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
                GeneralStep::ValidateApiInput => "validate-api-input",
                GeneralStep::RetrieveClusterConfig => "retrieve-cluster-config",
                GeneralStep::RetrieveClusterResources => "retrieve-cluster-resources",
                GeneralStep::ValidateSystemRequirements => "validate-system-requirements",
                GeneralStep::UnderMigration => "under-migration",
            }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// InfrastructureStep: represents an engine infrastructure step.
pub enum InfrastructureStep {
    /// LoadConfiguration: first step in infrastructure, aiming to load all configuration (from Terraform, etc).
    LoadConfiguration,
    /// Create: creating a cluster.
    Create,
    /// Created: cluster creation is ok.
    Created,
    /// CreateError: error on creating a cluster.
    CreateError,
    /// Pause: pausing a cluster.
    Pause,
    /// Paused: cluster pause is ok.
    Paused,
    /// PauseError: error on pausing a cluster.
    PauseError,
    /// Upgrade: upgrade a cluster.
    Upgrade,
    /// Upgraded: cluster upgrade is ok.
    Upgraded,
    /// Downgrade: downgrade a cluster.
    Downgrade,
    /// Downgraded: cluster downgrade is ok.
    Downgraded,
    /// Delete: delete a cluster.
    Delete,
    /// Deleted: cluster deletion is ok.
    Deleted,
    /// DeleteError: error on deleting a cluster.
    DeleteError,
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
                InfrastructureStep::Created => "created",
                InfrastructureStep::Paused => "paused",
                InfrastructureStep::Upgraded => "upgraded",
                InfrastructureStep::Downgraded => "downgraded",
                InfrastructureStep::Deleted => "deleted",
                InfrastructureStep::CreateError => "create-error",
                InfrastructureStep::PauseError => "pause-error",
                InfrastructureStep::DeleteError => "delete-error",
            },
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// EnvironmentStep: represents an engine environment step.
pub enum EnvironmentStep {
    /// LoadConfiguration: first step in environment, aiming to load all configuration (from Terraform, etc).
    LoadConfiguration,
    /// Build: building an application (docker or build packs).
    Build,
    /// Built: env is built.
    Built,
    /// Deploy: deploy an environment (application to kubernetes).
    Deploy,
    /// Deployed: env has been deployed.
    Deployed,
    /// Pause: pause an environment.
    Pause,
    /// Paused: env has been paused.
    Paused,
    /// Resume: resume a paused environment.
    Resume,
    /// Resumed: env has been resumed.
    Resumed,
    /// Update: update an environment.
    Update,
    /// Updated: env has been updated.
    Updated,
    /// Delete: delete an environment.
    Delete,
    /// Deleted: env has been deleted.
    Deleted,
    /// ScaleUp: scale up an environment.
    ScaleUp,
    /// ScaledUp: env has been scaled-up.
    ScaledUp,
    /// ScaleDown: scale down an environment.
    ScaleDown,
    /// ScaledDown: env has been scaled-down.
    ScaledDown,
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
                EnvironmentStep::Built => "built",
                EnvironmentStep::Deployed => "deployed",
                EnvironmentStep::Paused => "paused",
                EnvironmentStep::Resumed => "resumed",
                EnvironmentStep::Updated => "updated",
                EnvironmentStep::Deleted => "deleted",
                EnvironmentStep::ScaledUp => "scaled-up",
                EnvironmentStep::ScaledDown => "scaled-down",
            },
        )
    }
}

/// TransmitterId: represents a transmitter unique identifier.
type TransmitterId = String;
/// TransmitterName: represents a transmitter name.
type TransmitterName = String;
/// TransmitterType: represents a transmitter type.
type TransmitterType = String; // TODO(benjaminch): makes it a real enum / type
/// TransmitterVersion: represents a transmitter version.
type TransmitterVersion = String;

#[derive(Debug, Clone, PartialEq, Eq)]
/// Transmitter: represents the event's source caller (transmitter).
pub enum Transmitter {
    /// TaskManager: engine main task manager.
    TaskManager,
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
    Application(TransmitterId, TransmitterName, TransmitterVersion),
    /// Application: application engine part.
    Container(TransmitterId, TransmitterName, TransmitterVersion),
    /// Router: router engine part.
    Router(TransmitterId, TransmitterName),
    /// SecretManager: secret manager part
    SecretManager(TransmitterName),
}

impl Display for Transmitter {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match &self {
                Transmitter::TaskManager => "engine_task_manager".to_string(),
                Transmitter::BuildPlatform(id, name) => format!("build_platform({}, {})", id, name),
                Transmitter::ContainerRegistry(id, name) => format!("container_registry({}, {})", id, name),
                Transmitter::CloudProvider(id, name) => format!("cloud_provider({}, {})", id, name),
                Transmitter::Kubernetes(id, name) => format!("kubernetes({}, {})", id, name),
                Transmitter::DnsProvider(id, name) => format!("dns_provider({}, {})", id, name),
                Transmitter::ObjectStorage(id, name) => format!("object_strorage({}, {})", id, name),
                Transmitter::Environment(id, name) => format!("environment({}, {})", id, name),
                Transmitter::Database(id, db_type, name) => format!("database({}, {}, {})", id, db_type, name),
                Transmitter::Application(id, name, version) =>
                    format!("application({}, {}, commit: {})", id, name, version),
                Transmitter::Router(id, name) => format!("router({}, {})", id, name),
                Transmitter::SecretManager(name) => format!("secret_manager({})", name),
                Transmitter::Container(id, name, version) =>
                    format!("container({}, {}, version: {})", id, name, version),
            }
        )
    }
}

/// Region: represents event's cloud provider region.
type Region = String;

#[derive(Debug, Clone, PartialEq, Eq)]
/// EventDetails: represents an event details, carrying all useful data such as Qovery identifiers, transmitter, stage etc.
pub struct EventDetails {
    /// provider_kind: cloud provider name. an be set to None if not linked to any provider kind.
    provider_kind: Option<Kind>,
    /// organisation_id: Qovery organisation identifier.
    organisation_id: QoveryIdentifier,
    /// cluster_id: Qovery cluster identifier.
    cluster_id: QoveryIdentifier,
    /// execution_id: Qovery execution identifier.
    execution_id: String,
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
        execution_id: String,
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

    pub fn clone_changing_transmitter(event_details: EventDetails, transmitter: Transmitter) -> Self {
        let mut event_details = event_details;
        event_details.transmitter = transmitter;
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
    pub fn execution_id(&self) -> &str {
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
        struct TestCase {
            safe_message: String,
            raw_message: Option<String>,
            envs: Option<Vec<(String, String)>>,
            verbosity: EventMessageVerbosity,
            expected_output: String,
        }

        let test_cases: Vec<TestCase> = vec![
            TestCase {
                safe_message: "safe".to_string(),
                raw_message: Some("raw".to_string()),
                envs: Some(vec![("env".to_string(), "value".to_string())]),
                verbosity: EventMessageVerbosity::SafeOnly,
                expected_output: "safe".to_string(),
            },
            TestCase {
                safe_message: "safe".to_string(),
                raw_message: None,
                envs: None,
                verbosity: EventMessageVerbosity::SafeOnly,
                expected_output: "safe".to_string(),
            },
            TestCase {
                safe_message: "safe".to_string(),
                raw_message: None,
                envs: None,
                verbosity: EventMessageVerbosity::FullDetails,
                expected_output: "safe".to_string(),
            },
            TestCase {
                safe_message: "safe".to_string(),
                raw_message: Some("raw".to_string()),
                envs: None,
                verbosity: EventMessageVerbosity::FullDetails,
                expected_output: "safe / Full details: raw".to_string(),
            },
            TestCase {
                safe_message: "safe".to_string(),
                raw_message: Some("raw".to_string()),
                envs: Some(vec![("env".to_string(), "value".to_string())]),
                verbosity: EventMessageVerbosity::FullDetailsWithoutEnvVars,
                expected_output: "safe / Full details: raw".to_string(),
            },
            TestCase {
                safe_message: "safe".to_string(),
                raw_message: Some("raw".to_string()),
                envs: Some(vec![("env".to_string(), "value".to_string())]),
                verbosity: EventMessageVerbosity::FullDetails,
                expected_output: "safe / Full details: raw / Env vars: env=value".to_string(),
            },
        ];

        for tc in test_cases {
            // execute:
            let event_message = EventMessage::new_with_env_vars(tc.safe_message, tc.raw_message, tc.envs);

            // validate:
            assert_eq!(tc.expected_output, event_message.message(tc.verbosity));
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

    #[test]
    fn test_event_message_test_hidding_env_vars_in_message_safe_only() {
        // setup:
        let event_message = EventMessage::new_with_env_vars(
            "my safe message".to_string(),
            Some("my full message".to_string()),
            Some(vec![("my_secret".to_string(), "my_secret_value".to_string())]),
        );

        // execute:
        let res = event_message.message(EventMessageVerbosity::SafeOnly);

        // verify:
        assert!(!res.contains("my_secret"));
        assert!(!res.contains("my_secret_value"));
    }

    #[test]
    fn test_event_message_test_hidding_env_vars_in_message_full_without_env_vars() {
        // setup:
        let event_message = EventMessage::new_with_env_vars(
            "my safe message".to_string(),
            Some("my full message".to_string()),
            Some(vec![("my_secret".to_string(), "my_secret_value".to_string())]),
        );

        // execute:
        let res = event_message.message(EventMessageVerbosity::FullDetailsWithoutEnvVars);

        // verify:
        assert!(!res.contains("my_secret"));
        assert!(!res.contains("my_secret_value"));
    }

    #[test]
    fn test_event_message_test_hidding_env_vars_in_debug() {
        // setup:
        let event_message = EventMessage::new_with_env_vars(
            "my safe message".to_string(),
            Some("my full message".to_string()),
            Some(vec![("my_secret".to_string(), "my_secret_value".to_string())]),
        );

        // execute:
        let res = format!("{:?}", event_message);

        // verify:
        assert!(!res.contains("my_secret"));
        assert!(!res.contains("my_secret_value"));
    }
}

#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::large_enum_variant)]
#![allow(deprecated)]

pub mod io;

extern crate derivative;
extern crate url;

use crate::cloud_provider::Kind;
use crate::errors::{CommandError, EngineError, ErrorMessageVerbosity};
use crate::io_models::QoveryIdentifier;
use crate::metrics_registry::StepRecord;
use derivative::Derivative;
use std::fmt::{Display, Formatter};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum EngineMsgPayload {
    Metrics(StepRecord),
}

#[derive(Debug, Clone)]
pub struct EngineMsg {
    pub payload: EngineMsgPayload,
}

impl EngineMsg {
    pub fn new(payload: EngineMsgPayload) -> Self {
        EngineMsg { payload }
    }
}

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

    pub fn obfuscate(&mut self, transformer: impl Fn(String) -> String) {
        match self {
            EngineEvent::Debug(_, event_message) => {
                event_message.safe_message = transformer(std::mem::take(&mut event_message.safe_message));
                event_message.full_details = event_message.full_details.take().map(transformer)
            }
            EngineEvent::Info(_, event_message) => {
                event_message.safe_message = transformer(std::mem::take(&mut event_message.safe_message));
                event_message.full_details = event_message.full_details.take().map(transformer)
            }
            EngineEvent::Warning(_, event_message) => {
                event_message.safe_message = transformer(std::mem::take(&mut event_message.safe_message));
                event_message.full_details = event_message.full_details.take().map(transformer)
            }
            EngineEvent::Error(engine_error, Some(event_message)) => {
                engine_error.obfuscate(&transformer);
                event_message.safe_message = transformer(std::mem::take(&mut event_message.safe_message));
                event_message.full_details = event_message.full_details.take().map(transformer)
            }
            EngineEvent::Error(engine_error, None) => {
                engine_error.obfuscate(transformer);
            }
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

    pub fn transform(&mut self, transformer: impl Fn(String) -> String) {
        self.full_details = self.full_details.take().map(transformer);
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

    /// Creates e new EventMessage dedicated to transfer data to core
    ///
    /// Arguments
    ///
    /// * `safe_message`: Event safe message string (from which all unsafe text such as passwords and tokens has been removed).
    /// * `json_core_data`: The json representation of the data to be transfer to the core
    pub fn new_for_sending_core_data(safe_message: String, json: String) -> Self {
        EventMessage {
            safe_message,
            full_details: Some(json),
            env_vars: None,
        }
    }

    /// Creates e new EventMessage from engine error.
    ///
    /// Arguments
    ///
    /// * `engine_error`: Engine error.
    pub fn new_from_engine_error(engine_error: EngineError) -> Self {
        EventMessage {
            safe_message: engine_error.message(ErrorMessageVerbosity::SafeOnly),
            full_details: Some(engine_error.message(ErrorMessageVerbosity::FullDetailsWithoutEnvVars)),
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
                                .map(|(k, v)| format!("{k}={v}"))
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

    pub fn is_core_output(&self) -> bool {
        match self {
            Stage::Infrastructure(_) => false,
            Stage::Environment(step) => step.is_core_output(),
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

#[derive(Debug, Clone, PartialEq, Eq)]
/// InfrastructureStep: represents an engine infrastructure step.
pub enum InfrastructureStep {
    // general steps
    /// LoadConfiguration: first step in environment, aiming to load all configuration (from Terraform, etc).
    LoadConfiguration,
    /// ValidateApiInput: validating Engine's API input
    ValidateApiInput,
    /// ValidateSystemRequirements: validating system requirements
    ValidateSystemRequirements,
    /// RetrieveClusterConfig: retrieving cluster configuration
    RetrieveClusterConfig,
    /// RetrieveClusterResources: retrieving cluster resources
    RetrieveClusterResources,
    /// GlobalError: used to identify an error happening during a deployment step which is not linked to a specific deployment error step
    GlobalError,
    /// Deployment has started. It is the first message sent by the engine.
    Start,
    /// Deployment is terminated. It is the terminal message sent by the engine
    Terminated,

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
    /// UpgradedError: cluster upgrade is in error.
    UpgradeError,
    /// Delete: delete a cluster.
    Delete,
    /// Deleted: cluster deletion is ok.
    Deleted,
    /// DeleteError: error on deleting a cluster.
    DeleteError,
    /// Restart: restart a cluster
    Restart,
    /// Restarted: cluster restart is ok.
    Restarted,
    /// RestartedError: error on restarting a cluster.
    RestartedError,
    /// CannotProcessRequest: error returned if the payload sent is wrong
    CannotProcessRequest,
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
                InfrastructureStep::Created => "created",
                InfrastructureStep::Paused => "paused",
                InfrastructureStep::Upgraded => "upgraded",
                InfrastructureStep::UpgradeError => "upgrade-error",
                InfrastructureStep::Deleted => "deleted",
                InfrastructureStep::CreateError => "create-error",
                InfrastructureStep::PauseError => "pause-error",
                InfrastructureStep::DeleteError => "delete-error",
                InfrastructureStep::ValidateApiInput => "validate-api-input",
                InfrastructureStep::ValidateSystemRequirements => "validate-system-requirements",
                InfrastructureStep::RetrieveClusterConfig => "retrieve-cluster-config",
                InfrastructureStep::RetrieveClusterResources => "retrieve-cluster-resources",
                InfrastructureStep::Start => "start",
                InfrastructureStep::Terminated => "terminated",
                InfrastructureStep::Restart => "restart",
                InfrastructureStep::Restarted => "restarted",
                InfrastructureStep::RestartedError => "restart-error",
                InfrastructureStep::CannotProcessRequest => "cannot-process-request",
                InfrastructureStep::GlobalError => "global-error",
            },
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// EnvironmentStep: represents an engine environment step.
pub enum EnvironmentStep {
    // general steps
    /// Deployment has started. It is the first message sent by the engine.
    Start,
    /// Deployment is terminated. It is the terminal message sent by the engine
    Terminated,

    /// LoadConfiguration: first step in environment, aiming to load all configuration (from Terraform, etc).
    LoadConfiguration,
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
    /// GlobalError: used to identify an error happening during a deployment step which is not linked to a specific deployment error step
    GlobalError,

    // Env specific steps
    /// Build: building an application (docker or build packs).
    Build,
    /// Built: env is built.
    Built,
    /// BuiltError: Terminal error on building an application.
    BuiltError,
    // Environment received notification and is in progress to be cancelled.
    Cancel,
    // Environment deployment has been cancelled.
    Cancelled,
    /// Deploy: deploy an environment (application to kubernetes).
    Deploy,
    /// Deployed: env has been deployed.
    Deployed,
    /// DeployError: Terminal error on deploying an environment/service.
    DeployedError,
    /// Pause: pause an environment.
    Pause,
    /// Paused: env has been paused.
    Paused,
    /// PauseError: Terminal error on pausing an environment/service.
    PausedError,
    /// Delete: delete an environment.
    Delete,
    /// Deleted: env has been deleted.
    Deleted,
    /// DeleteError: Terminal error on deleting an environment/service.
    DeletedError,
    /// Recap: Display the error(s) recap of the whole service deployment
    Recap,
    /// Restart: Restart service pods
    Restart,
    /// Restarted: Service pods have been restarted
    Restarted,
    /// RestartedError: Error on restarting service pods
    RestartedError,

    // Transfer data to core
    /// JobOutput: contains the environment variables to upsert
    JobOutput,

    /// DatabaseOutput: contains the environment variables to upsert
    DatabaseOutput,
}

impl EnvironmentStep {
    pub fn is_error_step(&self) -> bool {
        matches!(
            self,
            EnvironmentStep::BuiltError
                | EnvironmentStep::Cancelled
                | EnvironmentStep::DeployedError
                | EnvironmentStep::PausedError
                | EnvironmentStep::DeletedError
                | EnvironmentStep::RestartedError
        )
    }

    pub fn is_core_output(&self) -> bool {
        matches!(self, EnvironmentStep::JobOutput | EnvironmentStep::DatabaseOutput)
    }
}

impl Display for EnvironmentStep {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match &self {
                EnvironmentStep::Build => "build",
                EnvironmentStep::Deploy => "deploy",
                EnvironmentStep::Delete => "delete",
                EnvironmentStep::Pause => "pause",
                EnvironmentStep::LoadConfiguration => "load-configuration",
                EnvironmentStep::Built => "built",
                EnvironmentStep::Deployed => "deployed",
                EnvironmentStep::Paused => "paused",
                EnvironmentStep::Deleted => "deleted",
                EnvironmentStep::Start => "start",
                EnvironmentStep::Cancel => "cancel",
                EnvironmentStep::Cancelled => "cancelled",
                EnvironmentStep::Terminated => "terminated",
                EnvironmentStep::BuiltError => "built-error",
                EnvironmentStep::DeployedError => "deployed-error",
                EnvironmentStep::PausedError => "paused-error",
                EnvironmentStep::DeletedError => "deleted-error",
                EnvironmentStep::ValidateApiInput => "validate-api-input",
                EnvironmentStep::ValidateSystemRequirements => "validate-system-requirements",
                EnvironmentStep::RetrieveClusterConfig => "retrieve-cluster-config",
                EnvironmentStep::RetrieveClusterResources => "retrieve-cluster-resources",
                EnvironmentStep::UnderMigration => "under-migration",
                EnvironmentStep::Restart => "restart",
                EnvironmentStep::Restarted => "restarted",
                EnvironmentStep::RestartedError => "restarted-error",
                EnvironmentStep::JobOutput => "job-output",
                EnvironmentStep::DatabaseOutput => "database-output",
                EnvironmentStep::Recap => "recap",
                EnvironmentStep::GlobalError => "global-error",
            },
        )
    }
}

/// TransmitterId: represents a transmitter unique identifier.
type TransmitterId = Uuid;
/// TransmitterName: represents a transmitter name.
type TransmitterName = String;

#[derive(Debug, Clone, PartialEq, Eq)]
/// Transmitter: represents the event's source caller (transmitter).
pub enum Transmitter {
    /// TaskManager: engine main task manager.
    TaskManager(TransmitterId, TransmitterName),
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
    Database(TransmitterId, TransmitterName),
    /// Application: application engine part.
    Application(TransmitterId, TransmitterName),
    /// Application: application engine part.
    Container(TransmitterId, TransmitterName),
    /// HelmChart: helmChart engine part.
    Helm(TransmitterId, TransmitterName),
    /// Router: router engine part.
    Router(TransmitterId, TransmitterName),
    /// Job: job engine part.
    Job(TransmitterId, TransmitterName),
}

impl Display for Transmitter {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match &self {
                Transmitter::TaskManager(id, name) => format!("engine_task_manager({id}, {name})"),
                Transmitter::BuildPlatform(id, name) => format!("build_platform({id}, {name})"),
                Transmitter::ContainerRegistry(id, name) => format!("container_registry({id}, {name})"),
                Transmitter::CloudProvider(id, name) => format!("cloud_provider({id}, {name})"),
                Transmitter::Kubernetes(id, name) => format!("kubernetes({id}, {name})"),
                Transmitter::DnsProvider(id, name) => format!("dns_provider({id}, {name})"),
                Transmitter::ObjectStorage(id, name) => format!("object_strorage({id}, {name})"),
                Transmitter::Environment(id, name) => format!("environment({id}, {name})"),
                Transmitter::Database(id, name) => format!("database({id}, {name})"),
                Transmitter::Application(id, name) => format!("application({id}, {name})"),
                Transmitter::Router(id, name) => format!("router({id}, {name})"),
                Transmitter::Container(id, name) => format!("container({id}, {name})"),
                Transmitter::Job(id, name) => format!("job({id}, {name})"),
                Transmitter::Helm(id, name) => format!("helm_chart({id}, {name})"),
            }
        )
    }
}

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
    /// * `stage`: Event's stage in which this event has been triggered.
    /// * `transmitter`: Event's source transmitter.
    pub fn new(
        provider_kind: Option<Kind>,
        organisation_id: QoveryIdentifier,
        cluster_id: QoveryIdentifier,
        execution_id: String,
        stage: Stage,
        transmitter: Transmitter,
    ) -> Self {
        EventDetails {
            provider_kind,
            organisation_id,
            cluster_id,
            execution_id,
            stage,
            transmitter,
        }
    }

    pub(super) fn mut_to_error_stage(&mut self) {
        self.stage = match &self.stage {
            Stage::Infrastructure(step) => match step {
                InfrastructureStep::Create | InfrastructureStep::Created => {
                    Stage::Infrastructure(InfrastructureStep::CreateError)
                }
                InfrastructureStep::Pause | InfrastructureStep::Paused => {
                    Stage::Infrastructure(InfrastructureStep::PauseError)
                }
                InfrastructureStep::Upgrade | InfrastructureStep::Upgraded => {
                    Stage::Infrastructure(InfrastructureStep::UpgradeError)
                }
                InfrastructureStep::Delete | InfrastructureStep::Deleted => {
                    Stage::Infrastructure(InfrastructureStep::DeleteError)
                }
                InfrastructureStep::Restart | InfrastructureStep::Restarted => {
                    Stage::Infrastructure(InfrastructureStep::RestartedError)
                }
                InfrastructureStep::LoadConfiguration
                | InfrastructureStep::ValidateApiInput
                | InfrastructureStep::ValidateSystemRequirements
                | InfrastructureStep::RetrieveClusterConfig
                | InfrastructureStep::RetrieveClusterResources
                | InfrastructureStep::GlobalError => Stage::Infrastructure(InfrastructureStep::GlobalError),
                InfrastructureStep::Start
                | InfrastructureStep::Terminated
                | InfrastructureStep::CreateError
                | InfrastructureStep::PauseError
                | InfrastructureStep::UpgradeError
                | InfrastructureStep::DeleteError
                | InfrastructureStep::RestartedError
                | InfrastructureStep::CannotProcessRequest => return,
            },
            Stage::Environment(step) => match step {
                EnvironmentStep::Build | EnvironmentStep::Built => Stage::Environment(EnvironmentStep::BuiltError),
                EnvironmentStep::Deploy | EnvironmentStep::Deployed => {
                    Stage::Environment(EnvironmentStep::DeployedError)
                }
                EnvironmentStep::Pause | EnvironmentStep::Paused => Stage::Environment(EnvironmentStep::PausedError),
                EnvironmentStep::Delete | EnvironmentStep::Deleted => Stage::Environment(EnvironmentStep::DeletedError),
                EnvironmentStep::Restart | EnvironmentStep::Restarted => {
                    Stage::Environment(EnvironmentStep::RestartedError)
                }
                EnvironmentStep::LoadConfiguration
                | EnvironmentStep::ValidateApiInput
                | EnvironmentStep::ValidateSystemRequirements
                | EnvironmentStep::RetrieveClusterConfig
                | EnvironmentStep::RetrieveClusterResources
                | EnvironmentStep::UnderMigration
                | EnvironmentStep::GlobalError => Stage::Environment(EnvironmentStep::GlobalError),
                EnvironmentStep::Start
                | EnvironmentStep::Terminated
                | EnvironmentStep::BuiltError
                | EnvironmentStep::Cancel
                | EnvironmentStep::Cancelled
                | EnvironmentStep::DeployedError
                | EnvironmentStep::PausedError
                | EnvironmentStep::DeletedError
                | EnvironmentStep::RestartedError
                | EnvironmentStep::JobOutput
                | EnvironmentStep::Recap
                | EnvironmentStep::DatabaseOutput => return,
            },
        };
    }

    pub(super) fn mut_to_cancel_stage(&mut self) {
        // We don't support cancel for infrastructure
        if let Stage::Environment(_) = &self.stage {
            self.stage = Stage::Environment(EnvironmentStep::Cancelled)
        }
    }

    pub(super) fn mut_to_recap_stage(&mut self) {
        // We don't support cancel for infrastructure
        if let Stage::Environment(_) = &self.stage {
            self.stage = Stage::Environment(EnvironmentStep::Recap)
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
    use crate::cloud_provider::Kind;
    use crate::errors::{CommandError, EngineError};
    use crate::events::{
        EngineEvent, EnvironmentStep, EventDetails, EventMessage, EventMessageVerbosity, InfrastructureStep, Stage,
        Transmitter,
    };
    use crate::io_models::QoveryIdentifier;
    use uuid::Uuid;

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
            (Stage::Environment(EnvironmentStep::Build), EnvironmentStep::Build.to_string()),
            (Stage::Environment(EnvironmentStep::Delete), EnvironmentStep::Delete.to_string()),
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
        let res = format!("{event_message:?}");

        // verify:
        assert!(!res.contains("my_secret"));
        assert!(!res.contains("my_secret_value"));
    }

    #[test]
    fn test_obfuscate_debug_event() {
        // setup:
        let txt_with_secret = "a txt with secret";
        let safe_message = "a txt with secret";

        let event_message = EventMessage::new_with_env_vars(
            safe_message.to_string(),
            Some(txt_with_secret.to_string()),
            Some(vec![("my_secret".to_string(), "my_secret_value".to_string())]),
        );

        let event_details = EventDetails {
            provider_kind: Some(Kind::Aws),
            organisation_id: QoveryIdentifier::new(Uuid::new_v4()),
            cluster_id: QoveryIdentifier::new(Uuid::new_v4()),
            execution_id: "ex".to_string(),
            stage: Stage::Environment(EnvironmentStep::Build),
            transmitter: Transmitter::Application(Uuid::new_v4(), "transmitter".to_string()),
        };

        let mut engine_event = EngineEvent::Debug(event_details.clone(), event_message.clone());

        // execute:
        engine_event.obfuscate(|txt| {
            if txt == *txt_with_secret {
                "xxx".to_string()
            } else {
                txt
            }
        });

        // verify:
        assert!(matches!(engine_event, EngineEvent::Debug(_, _)));
        if let EngineEvent::Debug(details, event) = engine_event {
            assert_eq!(details, event_details);
            assert_eq!(event.full_details, Some("xxx".to_string()));
            assert_eq!(event.safe_message, "xxx");
        }
    }

    #[test]
    fn test_obfuscate_info_event() {
        // setup:
        let txt_with_secret = "a txt with secret";
        let safe_message = "a txt with secret";

        let event_message = EventMessage::new_with_env_vars(
            safe_message.to_string(),
            Some(txt_with_secret.to_string()),
            Some(vec![("my_secret".to_string(), "my_secret_value".to_string())]),
        );

        let event_details = EventDetails {
            provider_kind: Some(Kind::Aws),
            organisation_id: QoveryIdentifier::new(Uuid::new_v4()),
            cluster_id: QoveryIdentifier::new(Uuid::new_v4()),
            execution_id: "ex".to_string(),
            stage: Stage::Environment(EnvironmentStep::Build),
            transmitter: Transmitter::Application(Uuid::new_v4(), "transmitter".to_string()),
        };

        let mut engine_event = EngineEvent::Info(event_details.clone(), event_message.clone());

        // execute:
        engine_event.obfuscate(|txt| {
            if txt == *txt_with_secret {
                "xxx".to_string()
            } else {
                txt
            }
        });

        // verify:
        assert!(matches!(engine_event, EngineEvent::Info(_, _)));
        if let EngineEvent::Info(details, event) = engine_event {
            assert_eq!(details, event_details);
            assert_eq!(event.full_details, Some("xxx".to_string()));
            assert_eq!(event.safe_message, "xxx".to_string());
        }
    }

    #[test]
    fn test_obfuscate_warning_event() {
        // setup:
        let txt_with_secret = "a txt with secret";
        let safe_message = "a txt with secret";

        let event_message = EventMessage::new_with_env_vars(
            safe_message.to_string(),
            Some(txt_with_secret.to_string()),
            Some(vec![("my_secret".to_string(), "my_secret_value".to_string())]),
        );

        let event_details = EventDetails {
            provider_kind: Some(Kind::Aws),
            organisation_id: QoveryIdentifier::new(Uuid::new_v4()),
            cluster_id: QoveryIdentifier::new(Uuid::new_v4()),
            execution_id: "ex".to_string(),
            stage: Stage::Environment(EnvironmentStep::Build),
            transmitter: Transmitter::Application(Uuid::new_v4(), "transmitter".to_string()),
        };

        let mut engine_event = EngineEvent::Warning(event_details.clone(), event_message.clone());

        // execute:
        engine_event.obfuscate(|txt| {
            if txt == *txt_with_secret {
                "xxx".to_string()
            } else {
                txt
            }
        });

        // verify:
        assert!(matches!(engine_event, EngineEvent::Warning(_, _)));
        if let EngineEvent::Warning(details, event) = engine_event {
            assert_eq!(details, event_details);
            assert_eq!(event.full_details, Some("xxx".to_string()));
            assert_eq!(event.safe_message, "xxx".to_string());
        }
    }

    #[test]
    fn test_obfuscate_error_event() {
        // setup:
        let txt_with_secret = "a txt with secret";
        let safe_message = "a txt with secret";

        let event_message = EventMessage::new_with_env_vars(
            safe_message.to_string(),
            Some(txt_with_secret.to_string()),
            Some(vec![("my_secret".to_string(), "my_secret_value".to_string())]),
        );

        let event_details = EventDetails {
            provider_kind: Some(Kind::Aws),
            organisation_id: QoveryIdentifier::new(Uuid::new_v4()),
            cluster_id: QoveryIdentifier::new(Uuid::new_v4()),
            execution_id: "ex".to_string(),
            stage: Stage::Environment(EnvironmentStep::BuiltError),
            transmitter: Transmitter::Application(Uuid::new_v4(), "transmitter".to_string()),
        };

        let engine_error = EngineError::new_unknown(
            event_details.clone(),
            txt_with_secret.to_string(),
            Some(CommandError::new(
                safe_message.to_string(),
                Some(txt_with_secret.to_string()),
                Some(vec![("my_secret".to_string(), "my_secret_value".to_string())]),
            )),
            None,
            Some(txt_with_secret.to_string()),
        );

        let mut engine_event = EngineEvent::Error(engine_error.clone(), Some(event_message.clone()));

        // execute:
        engine_event.obfuscate(|txt| {
            if txt == *txt_with_secret {
                "xxx".to_string()
            } else {
                txt
            }
        });

        // verify:
        assert!(matches!(engine_event, EngineEvent::Error(_, _)));
        if let EngineEvent::Error(engine_error, Some(event)) = engine_event {
            assert_eq!(event.full_details, Some("xxx".to_string()));
            assert_eq!(event.safe_message, "xxx");

            assert_eq!(engine_error.hint_message().clone().unwrap_or_default(), "xxx".to_string());
            assert_eq!(engine_error.user_log_message().to_string(), "xxx".to_string());
            assert_eq!(
                engine_error
                    .underlying_error()
                    .unwrap()
                    .message_raw()
                    .unwrap_or_default(),
                "xxx".to_string()
            );
            assert_eq!(engine_error.event_details(), &event_details);
        }
    }
}

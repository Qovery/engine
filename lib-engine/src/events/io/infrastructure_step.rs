use serde::{Deserialize, Serialize};

use crate::events;

#[derive(Deserialize, Serialize)]
pub enum InfrastructureStep {
    LoadConfiguration,
    Create,
    Created,
    CreateError,
    Pause,
    Paused,
    PauseError,
    Upgrade,
    Upgraded,
    UpgradeError,
    Delete,
    Deleted,
    DeleteError,
    ValidateApiInput,
    ValidateSystemRequirements,
    RetrieveClusterConfig,
    RetrieveClusterResources,
    Start,
    Terminated,
    Restart,
    Restarted,
    RestartedError,
    CannotProcessRequest,
    GlobalError,
    InfrastructureDiff,
    PlatformExecutionResult,
}

impl From<events::InfrastructureStep> for InfrastructureStep {
    fn from(step: events::InfrastructureStep) -> Self {
        match step {
            events::InfrastructureStep::LoadConfiguration => InfrastructureStep::LoadConfiguration,
            events::InfrastructureStep::Create => InfrastructureStep::Create,
            events::InfrastructureStep::Pause => InfrastructureStep::Pause,
            events::InfrastructureStep::Upgrade => InfrastructureStep::Upgrade,
            events::InfrastructureStep::Delete => InfrastructureStep::Delete,
            events::InfrastructureStep::Created => InfrastructureStep::Created,
            events::InfrastructureStep::Paused => InfrastructureStep::Paused,
            events::InfrastructureStep::Upgraded => InfrastructureStep::Upgraded,
            events::InfrastructureStep::Deleted => InfrastructureStep::Deleted,
            events::InfrastructureStep::CreateError => InfrastructureStep::CreateError,
            events::InfrastructureStep::PauseError => InfrastructureStep::PauseError,
            events::InfrastructureStep::DeleteError => InfrastructureStep::DeleteError,
            events::InfrastructureStep::ValidateApiInput => InfrastructureStep::ValidateApiInput,
            events::InfrastructureStep::ValidateSystemRequirements => InfrastructureStep::ValidateSystemRequirements,
            events::InfrastructureStep::RetrieveClusterConfig => InfrastructureStep::RetrieveClusterConfig,
            events::InfrastructureStep::RetrieveClusterResources => InfrastructureStep::RetrieveClusterResources,
            events::InfrastructureStep::Start => InfrastructureStep::Start,
            events::InfrastructureStep::Terminated => InfrastructureStep::Terminated,
            events::InfrastructureStep::UpgradeError => InfrastructureStep::UpgradeError,
            events::InfrastructureStep::Restart => InfrastructureStep::Restart,
            events::InfrastructureStep::Restarted => InfrastructureStep::Restarted,
            events::InfrastructureStep::RestartedError => InfrastructureStep::RestartedError,
            events::InfrastructureStep::CannotProcessRequest => InfrastructureStep::CannotProcessRequest,
            events::InfrastructureStep::GlobalError => InfrastructureStep::GlobalError,
            events::InfrastructureStep::InfrastructureDiff(_) => InfrastructureStep::InfrastructureDiff,
            events::InfrastructureStep::PlatformExecutionResult => InfrastructureStep::PlatformExecutionResult,
        }
    }
}

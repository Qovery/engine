use serde::{Deserialize, Serialize};

use crate::events;

#[derive(Deserialize, Serialize)]
pub enum BlueprintStep {
    Cancel,
    Cancelled,
    Delete,
    Deploy,
    Deployed,
    DeployedError,
    LoadConfiguration,
    Pause,
    Restart,
    Start,
    Terminated,
}

impl From<events::BlueprintStep> for BlueprintStep {
    fn from(value: events::BlueprintStep) -> Self {
        match value {
            events::BlueprintStep::Cancel => BlueprintStep::Cancel,
            events::BlueprintStep::Cancelled => BlueprintStep::Cancelled,
            events::BlueprintStep::Delete => BlueprintStep::Delete,
            events::BlueprintStep::Deploy => BlueprintStep::Deploy,
            events::BlueprintStep::Deployed => BlueprintStep::Deployed,
            events::BlueprintStep::DeployedError => BlueprintStep::DeployedError,
            events::BlueprintStep::LoadConfiguration => BlueprintStep::LoadConfiguration,
            events::BlueprintStep::Pause => BlueprintStep::Pause,
            events::BlueprintStep::Restart => BlueprintStep::Restart,
            events::BlueprintStep::Start => BlueprintStep::Start,
            events::BlueprintStep::Terminated => BlueprintStep::Terminated,
        }
    }
}

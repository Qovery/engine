use serde::{Deserialize, Serialize};

use crate::events;

#[derive(Deserialize, Serialize)]
pub enum ClusterAnalysisStep {
    Start,
    CostRecommendation,
    DeprecatedApiCheck,
    Succeeded,
    Error,
    Terminated,
}

impl From<events::ClusterAnalysisStep> for ClusterAnalysisStep {
    fn from(step: events::ClusterAnalysisStep) -> Self {
        match step {
            events::ClusterAnalysisStep::Start => ClusterAnalysisStep::Start,
            events::ClusterAnalysisStep::CostRecommendation => ClusterAnalysisStep::CostRecommendation,
            events::ClusterAnalysisStep::DeprecatedApiCheck => ClusterAnalysisStep::DeprecatedApiCheck,
            events::ClusterAnalysisStep::Succeeded => ClusterAnalysisStep::Succeeded,
            events::ClusterAnalysisStep::Error => ClusterAnalysisStep::Error,
            events::ClusterAnalysisStep::Terminated => ClusterAnalysisStep::Terminated,
        }
    }
}

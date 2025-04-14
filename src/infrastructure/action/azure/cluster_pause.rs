use crate::errors::EngineError;
use crate::infrastructure::action::InfraLogger;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::kubernetes::azure::aks::AKS;

pub(super) fn pause_aks_cluster(
    _cluster: &AKS,
    _infra_ctx: &InfrastructureContext,
    logger: impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    logger.warn("Pausing a AKS cluster is not supported yet. Skipping this step.");

    // TODO(benjaminch): implement cluster pause

    Ok(())
}

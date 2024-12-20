use crate::errors::EngineError;
use crate::infrastructure::action::InfraLogger;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::kubernetes::gcp::Gke;

pub(super) fn pause_gke_cluster(
    cluster: &Gke,
    infra_ctx: &InfrastructureContext,
    logger: impl InfraLogger,
) -> Result<(), Box<EngineError>> {
    logger.warn("Pausing a GKE cluster is not supported yet. Skipping this step.");
    // Configure kubectl to be able to connect to cluster
    let _ = cluster.configure_gcloud_for_cluster(infra_ctx); // TODO(ENG-1802): properly handle this error

    Ok(())
}

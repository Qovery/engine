use crate::cloud_provider::gcp::kubernetes::Gke;
use crate::engine::InfrastructureContext;
use crate::errors::EngineError;
use crate::infrastructure_action::InfraLogger;

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

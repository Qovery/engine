use crate::cloud_provider::gcp::kubernetes::Gke;
use crate::engine::InfrastructureContext;
use crate::errors::EngineError;

pub(super) fn pause_gke_cluster(cluster: &Gke, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
    // Configure kubectl to be able to connect to cluster
    let _ = cluster.configure_gcloud_for_cluster(infra_ctx); // TODO(ENG-1802): properly handle this error

    Ok(())
}

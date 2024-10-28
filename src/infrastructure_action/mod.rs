mod ec2_k3s;
mod eks;
mod gke;
mod scaleway;
mod self_managed;
mod utils;

use crate::cloud_provider::kubernetes::KubernetesUpgradeStatus;
use crate::engine::InfrastructureContext;
use crate::errors::EngineError;

// TODO: Remove pub export if possible
pub use ec2_k3s::AwsEc2QoveryTerraformOutput;

pub trait InfrastructureAction: Send + Sync {
    fn create_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>>;
    fn pause_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>>;
    fn delete_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>>;
    fn upgrade_cluster(
        &self,
        infra_ctx: &InfrastructureContext,
        kubernetes_upgrade_status: KubernetesUpgradeStatus,
    ) -> Result<(), Box<EngineError>>;
}

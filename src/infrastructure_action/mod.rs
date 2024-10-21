mod ec2_k3s;
pub mod eks;
mod utils;

use crate::cloud_provider::kubernetes::KubernetesUpgradeStatus;
use crate::engine::InfrastructureContext;
use crate::errors::EngineError;

// TODO: Remove pub export if possible
pub use ec2_k3s::AwsEc2QoveryTerraformOutput;

pub trait InfrastructureAction: Send + Sync {
    fn on_create_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>>;
    fn on_pause_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>>;
    fn on_delete_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>>;
    fn on_upgrade_cluster(
        &self,
        infra_ctx: &InfrastructureContext,
        kubernetes_upgrade_status: KubernetesUpgradeStatus,
    ) -> Result<(), Box<EngineError>>;
}

use crate::cloud_provider::kubernetes::{Kubernetes, KubernetesUpgradeStatus};
use crate::cloud_provider::self_managed::kubernetes::SelfManaged;
use crate::engine::InfrastructureContext;
use crate::errors::EngineError;
use crate::events::InfrastructureStep;
use crate::events::Stage::Infrastructure;
use crate::infrastructure_action::InfrastructureAction;

impl InfrastructureAction for SelfManaged {
    fn create_cluster(&self, _infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        Err(Box::new(EngineError::new_cannot_restart_kubernetes_cluster(
            self.get_event_details(Infrastructure(InfrastructureStep::CreateError)),
        )))
    }

    fn pause_cluster(&self, _infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        Err(Box::new(EngineError::new_cannot_restart_kubernetes_cluster(
            self.get_event_details(Infrastructure(InfrastructureStep::Pause)),
        )))
    }

    fn delete_cluster(&self, _infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        Err(Box::new(EngineError::new_cannot_restart_kubernetes_cluster(
            self.get_event_details(Infrastructure(InfrastructureStep::Delete)),
        )))
    }

    fn upgrade_cluster(
        &self,
        _infra_ctx: &InfrastructureContext,
        _kubernetes_upgrade_status: KubernetesUpgradeStatus,
    ) -> Result<(), Box<EngineError>> {
        Err(Box::new(EngineError::new_cannot_restart_kubernetes_cluster(
            self.get_event_details(Infrastructure(InfrastructureStep::Upgrade)),
        )))
    }
}

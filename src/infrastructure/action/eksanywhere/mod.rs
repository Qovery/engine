mod cluster_install;
mod helm_charts;

use crate::errors::EngineError;
use crate::events::InfrastructureStep;
use crate::events::Stage::Infrastructure;
use crate::infrastructure::action::InfrastructureAction;
use crate::infrastructure::action::eksanywhere::cluster_install::install_eks_anywhere_charts;
use crate::infrastructure::action::utils::mk_logger;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::cloud_provider::service::Action;
use crate::infrastructure::models::kubernetes::eksanywhere::EksAnywhere;
use crate::infrastructure::models::kubernetes::{Kubernetes, KubernetesUpgradeStatus, send_progress_on_long_task};

impl InfrastructureAction for EksAnywhere {
    fn create_cluster(
        &self,
        infra_ctx: &InfrastructureContext,
        _has_been_upgraded: bool,
    ) -> Result<(), Box<EngineError>> {
        let logger = mk_logger(infra_ctx.kubernetes(), InfrastructureStep::Create);
        send_progress_on_long_task(self, Action::Create, || install_eks_anywhere_charts(self, infra_ctx, logger))
    }

    fn pause_cluster(&self, _infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        Err(Box::new(EngineError::new_cannot_restart_kubernetes_cluster(
            self.get_event_details(Infrastructure(InfrastructureStep::PauseError)),
        )))
    }

    fn delete_cluster(&self, _infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        Err(Box::new(EngineError::new_cannot_restart_kubernetes_cluster(
            self.get_event_details(Infrastructure(InfrastructureStep::DeleteError)),
        )))
    }

    fn upgrade_cluster(
        &self,
        _infra_ctx: &InfrastructureContext,
        _kubernetes_upgrade_status: KubernetesUpgradeStatus,
    ) -> Result<(), Box<EngineError>> {
        Err(Box::new(EngineError::new_cannot_restart_kubernetes_cluster(
            self.get_event_details(Infrastructure(InfrastructureStep::UpgradeError)),
        )))
    }
}

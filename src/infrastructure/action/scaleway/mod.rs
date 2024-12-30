use crate::errors::EngineError;
use crate::events::InfrastructureStep;
use crate::infrastructure::action::scaleway::cluster_create::create_kapsule_cluster;
use crate::infrastructure::action::scaleway::cluster_delete::delete_kapsule_cluster;
use crate::infrastructure::action::scaleway::cluster_pause::pause_kapsule_cluster;
use crate::infrastructure::action::scaleway::cluster_upgrade::upgrade_kapsule_cluster;
use crate::infrastructure::action::InfrastructureAction;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::cloud_provider::service::Action;
use crate::infrastructure::models::kubernetes::scaleway::kapsule::Kapsule;
use crate::infrastructure::models::kubernetes::{send_progress_on_long_task, KubernetesUpgradeStatus};
use serde_derive::{Deserialize, Serialize};

mod cluster_create;
mod cluster_delete;
mod cluster_pause;
mod cluster_upgrade;
mod helm_charts;
mod nodegroup;
mod tera_context;

impl InfrastructureAction for Kapsule {
    fn create_cluster(
        &self,
        infra_ctx: &InfrastructureContext,
        _has_been_upraded: bool,
    ) -> Result<(), Box<EngineError>> {
        let logger = mk_logger(infra_ctx.kubernetes(), InfrastructureStep::Create);
        send_progress_on_long_task(self, Action::Create, || create_kapsule_cluster(self, infra_ctx, logger))
    }

    fn pause_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        let logger = mk_logger(infra_ctx.kubernetes(), InfrastructureStep::Pause);
        send_progress_on_long_task(self, Action::Pause, || pause_kapsule_cluster(self, infra_ctx, logger))
    }

    fn delete_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        let logger = mk_logger(infra_ctx.kubernetes(), InfrastructureStep::Delete);
        send_progress_on_long_task(self, Action::Delete, || delete_kapsule_cluster(self, infra_ctx, logger))
    }

    fn upgrade_cluster(
        &self,
        infra_ctx: &InfrastructureContext,
        kubernetes_upgrade_status: KubernetesUpgradeStatus,
    ) -> Result<(), Box<EngineError>> {
        let logger = mk_logger(infra_ctx.kubernetes(), InfrastructureStep::Upgrade);
        send_progress_on_long_task(self, Action::Create, || {
            upgrade_kapsule_cluster(self, infra_ctx, kubernetes_upgrade_status, logger)
        })
    }
}

use super::utils::{from_terraform_value, mk_logger};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScalewayQoveryTerraformOutput {
    #[serde(deserialize_with = "from_terraform_value")]
    pub loki_storage_config_scaleway_s3: String,
    #[serde(deserialize_with = "from_terraform_value")]
    pub kubeconfig: String,
}

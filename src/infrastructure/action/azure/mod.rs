mod cluster_create;
mod cluster_delete;
mod cluster_pause;
mod cluster_upgrade;
pub(crate) mod helm_charts;
mod tera_context;

use super::utils::{from_terraform_value, mk_logger};
use crate::errors::EngineError;
use crate::events::InfrastructureStep;
use crate::infrastructure::action::InfrastructureAction;
use crate::infrastructure::action::azure::cluster_create::create_aks_cluster;
use crate::infrastructure::action::azure::cluster_delete::delete_aks_cluster;
use crate::infrastructure::action::azure::cluster_pause::pause_aks_cluster;
use crate::infrastructure::action::azure::cluster_upgrade::upgrade_aks_cluster;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::cloud_provider::service::Action;
use crate::infrastructure::models::kubernetes::azure::aks::AKS;
use crate::infrastructure::models::kubernetes::{KubernetesUpgradeStatus, send_progress_on_long_task};
use serde_derive::{Deserialize, Serialize};

impl InfrastructureAction for AKS {
    fn create_cluster(
        &self,
        infra_ctx: &InfrastructureContext,
        _has_been_upgraded: bool,
    ) -> Result<(), Box<EngineError>> {
        let logger = mk_logger(infra_ctx.kubernetes(), InfrastructureStep::Create);
        send_progress_on_long_task(self, Action::Create, || create_aks_cluster(self, infra_ctx, logger))
    }

    fn pause_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        let logger = mk_logger(infra_ctx.kubernetes(), InfrastructureStep::Pause);
        send_progress_on_long_task(self, Action::Pause, || pause_aks_cluster(self, infra_ctx, logger))
    }

    fn delete_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        let logger = mk_logger(infra_ctx.kubernetes(), InfrastructureStep::Delete);
        send_progress_on_long_task(self, Action::Delete, || delete_aks_cluster(self, infra_ctx, logger))
    }

    fn upgrade_cluster(
        &self,
        infra_ctx: &InfrastructureContext,
        kubernetes_upgrade_status: KubernetesUpgradeStatus,
    ) -> Result<(), Box<EngineError>> {
        let logger = mk_logger(infra_ctx.kubernetes(), InfrastructureStep::Upgrade);

        send_progress_on_long_task(self, Action::Create, || {
            upgrade_aks_cluster(self, infra_ctx, kubernetes_upgrade_status, logger)
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AksQoveryTerraformOutput {
    #[serde(deserialize_with = "from_terraform_value")]
    pub aks_cluster_public_hostname: String,
    #[serde(deserialize_with = "from_terraform_value")]
    #[serde(default)]
    pub main_storage_account_name: String,
    #[serde(deserialize_with = "from_terraform_value")]
    #[serde(default)]
    pub main_storage_account_primary_access_key: String,
    #[serde(deserialize_with = "from_terraform_value")]
    #[serde(default)]
    pub loki_logging_service_msi_client_id: String,
    #[serde(deserialize_with = "from_terraform_value")]
    pub kubeconfig: String,
    #[serde(deserialize_with = "from_terraform_value")]
    pub cluster_name: String,
    #[serde(deserialize_with = "from_terraform_value")]
    pub cluster_id: String,
    #[serde(deserialize_with = "from_terraform_value")]
    pub cluster_oidc_issuer: String,
}

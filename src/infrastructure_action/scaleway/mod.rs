use crate::cloud_provider::kubernetes::{send_progress_on_long_task, Kubernetes, KubernetesUpgradeStatus};
use crate::cloud_provider::scaleway::kubernetes::Kapsule;
use crate::cloud_provider::service::Action;
use crate::cloud_provider::utilities::print_action;
use crate::engine::InfrastructureContext;
use crate::errors::EngineError;
use crate::events::InfrastructureStep;
use crate::events::Stage::Infrastructure;
use crate::infrastructure_action::scaleway::cluster_create::create_kapsule_cluster;
use crate::infrastructure_action::scaleway::cluster_delete::delete_kapsule_cluster;
use crate::infrastructure_action::scaleway::cluster_pause::pause_kapsule_cluster;
use crate::infrastructure_action::scaleway::cluster_upgrade::upgrade_kapsule_cluster;
use crate::infrastructure_action::InfrastructureAction;
use function_name::named;
use serde_derive::{Deserialize, Serialize};

mod cluster_create;
mod cluster_delete;
mod cluster_pause;
mod cluster_upgrade;
mod helm_charts;
mod nodegroup;
mod tera_context;

impl InfrastructureAction for Kapsule {
    #[named]
    fn create_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Create));
        print_action(
            infra_ctx.cloud_provider().name(),
            "kubernetes",
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Create, || create_kapsule_cluster(self, infra_ctx))
    }

    #[named]
    fn pause_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Pause));
        print_action(
            infra_ctx.cloud_provider().name(),
            "kubernetes",
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Pause, || pause_kapsule_cluster(self, infra_ctx))
    }

    #[named]
    fn delete_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Delete));
        print_action(
            infra_ctx.cloud_provider().name(),
            "kubernetes",
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Delete, || delete_kapsule_cluster(self, infra_ctx))
    }

    #[named]
    fn upgrade_cluster(
        &self,
        infra_ctx: &InfrastructureContext,
        kubernetes_upgrade_status: KubernetesUpgradeStatus,
    ) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Upgrade));
        print_action(
            infra_ctx.cloud_provider().name(),
            "kubernetes",
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        upgrade_kapsule_cluster(self, infra_ctx, kubernetes_upgrade_status)
    }
}

use super::utils::from_terraform_value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScalewayQoveryTerraformOutput {
    #[serde(deserialize_with = "from_terraform_value")]
    pub loki_storage_config_scaleway_s3: String,
}

mod cluster_create;
mod cluster_delete;
mod cluster_pause;
mod cluster_upgrade;
mod helm_charts;
mod tera_context;

use crate::cloud_provider::gcp::kubernetes::Gke;
use crate::cloud_provider::kubernetes::{send_progress_on_long_task, Kubernetes, KubernetesUpgradeStatus};
use crate::cloud_provider::service::Action;
use crate::cloud_provider::utilities::print_action;
use crate::engine::InfrastructureContext;
use crate::errors::EngineError;
use crate::events::InfrastructureStep;
use crate::events::Stage::Infrastructure;
use crate::infrastructure_action::gke::cluster_create::create_gke_cluster;
use crate::infrastructure_action::gke::cluster_delete::delete_gke_cluster;
use crate::infrastructure_action::gke::cluster_pause::pause_gke_cluster;
use crate::infrastructure_action::gke::cluster_upgrade::upgrade_gke_cluster;
use crate::infrastructure_action::InfrastructureAction;
use function_name::named;
use serde_derive::{Deserialize, Serialize};

impl InfrastructureAction for Gke {
    #[named]
    fn create_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Create));
        print_action(
            infra_ctx.cloud_provider().kind().to_string().to_lowercase().as_str(),
            "kubernetes",
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Create, || create_gke_cluster(self, infra_ctx))
    }

    #[named]
    fn pause_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Delete));
        print_action(
            infra_ctx.cloud_provider().kind().to_string().to_lowercase().as_str(),
            "kubernetes",
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Delete, || pause_gke_cluster(self, infra_ctx))
    }

    #[named]
    fn delete_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Delete));
        print_action(
            infra_ctx.cloud_provider().kind().to_string().to_lowercase().as_str(),
            "kubernetes",
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Delete, || delete_gke_cluster(self, infra_ctx))
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
        upgrade_gke_cluster(self, infra_ctx, kubernetes_upgrade_status)
    }
}

use super::utils::from_terraform_value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GkeQoveryTerraformOutput {
    #[serde(deserialize_with = "from_terraform_value")]
    pub gke_cluster_public_hostname: String,
    #[serde(deserialize_with = "from_terraform_value")]
    #[serde(default)]
    pub loki_logging_service_account_email: String,
}

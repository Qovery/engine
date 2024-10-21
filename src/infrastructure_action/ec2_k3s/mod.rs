use crate::cloud_provider::aws::kubernetes::ec2::EC2;
use crate::cloud_provider::kubernetes::{send_progress_on_long_task, Kubernetes, KubernetesUpgradeStatus};
use crate::cloud_provider::service::Action;
use crate::cloud_provider::utilities::print_action;
use crate::engine::InfrastructureContext;
use crate::errors::EngineError;
use crate::events::InfrastructureStep;
use crate::events::Stage::Infrastructure;
use crate::infrastructure_action::ec2_k3s::cluster_upgrade::ec2_k3s_cluster_upgrade;
use crate::infrastructure_action::InfrastructureAction;
use function_name::named;
use serde_derive::{Deserialize, Serialize};

mod cluster_upgrade;
// super is required because it used by the action of eks ;x
pub(super) mod helm_charts;
pub(super) mod sdk;
pub(super) mod utils;

use super::utils::from_terraform_value;

impl InfrastructureAction for EC2 {
    #[named]
    fn on_create_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Create));
        print_action(
            infra_ctx.cloud_provider().name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Create, || {
            crate::infrastructure_action::eks::cluster_create::create_eks_cluster(
                infra_ctx,
                self,
                infra_ctx.cloud_provider(),
                infra_ctx.dns_provider(),
                &self.s3,
                self.long_id,
                self.template_directory.as_str(),
                &self.zones,
                &[self.node_group_from_instance_type()],
                &self.options,
                &self.advanced_settings,
                None,
            )
        })
    }

    #[named]
    fn on_pause_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Pause));
        print_action(
            infra_ctx.cloud_provider().name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Pause, || {
            crate::infrastructure_action::eks::cluster_pause::pause_eks_cluster(
                infra_ctx,
                self,
                infra_ctx.cloud_provider(),
                infra_ctx.dns_provider(),
                self.template_directory.as_str(),
                &self.zones,
                &[self.node_group_from_instance_type()],
                &self.options,
                &self.advanced_settings,
                None,
            )
        })
    }

    #[named]
    fn on_delete_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Delete));
        print_action(
            infra_ctx.cloud_provider().name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Delete, || {
            crate::infrastructure_action::eks::cluster_delete::delete_eks_cluster(
                infra_ctx,
                self,
                infra_ctx.cloud_provider(),
                infra_ctx.dns_provider(),
                &self.s3,
                self.template_directory.as_str(),
                &self.zones,
                &[self.node_group_from_instance_type()],
                &self.options,
                &self.advanced_settings,
                None,
            )
        })
    }

    #[named]
    fn on_upgrade_cluster(
        &self,
        infra_ctx: &InfrastructureContext,
        kubernetes_upgrade_status: KubernetesUpgradeStatus,
    ) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Upgrade));
        print_action(
            infra_ctx.cloud_provider().name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        ec2_k3s_cluster_upgrade(self, infra_ctx, kubernetes_upgrade_status)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AwsEc2QoveryTerraformOutput {
    #[serde(deserialize_with = "from_terraform_value")]
    pub aws_ec2_public_hostname: String,
    #[serde(deserialize_with = "from_terraform_value")]
    pub aws_ec2_kubernetes_port: String,
    #[serde(deserialize_with = "from_terraform_value")]
    pub aws_aws_account_id: String,
    #[serde(deserialize_with = "from_terraform_value")]
    pub aws_iam_alb_controller_arn: String,
}

impl AwsEc2QoveryTerraformOutput {
    pub fn kubernetes_port_to_u16(&self) -> Result<u16, String> {
        match self.aws_ec2_kubernetes_port.parse::<u16>() {
            Ok(x) => Ok(x),
            Err(e) => Err(format!(
                "error while trying to convert kubernetes port from string {} to int: {}",
                self.aws_ec2_kubernetes_port, e
            )),
        }
    }
}

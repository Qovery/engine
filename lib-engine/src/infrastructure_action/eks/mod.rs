pub(super) mod cluster_create;
pub(super) mod cluster_delete;
pub(super) mod cluster_pause;
mod cluster_upgrade;
mod custom_vpc;
mod helm_charts;
mod karpenter;
mod nodegroup;
mod sdk;
mod tera_context;
mod utils;

// used by ec2_k3s/cluster_upgrade.rs
pub use tera_context::eks_tera_context;

use crate::cloud_provider::aws::kubernetes::eks::EKS;
use crate::cloud_provider::kubernetes::{send_progress_on_long_task, Kubernetes, KubernetesUpgradeStatus};
use crate::cloud_provider::service::Action;
use crate::cloud_provider::utilities::print_action;
use crate::engine::InfrastructureContext;
use crate::errors::EngineError;
use crate::events::InfrastructureStep;
use crate::events::Stage::Infrastructure;
use crate::infrastructure_action::eks::cluster_create::create_eks_cluster;
use crate::infrastructure_action::eks::cluster_delete::delete_eks_cluster;
use crate::infrastructure_action::eks::cluster_pause::pause_eks_cluster;
use crate::infrastructure_action::eks::cluster_upgrade::upgrade_eks_cluster;
use crate::infrastructure_action::InfrastructureAction;
use chrono::Duration as ChronoDuration;
use function_name::named;
use serde_derive::{Deserialize, Serialize};

static AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION: ChronoDuration = ChronoDuration::hours(1);
// https://docs.aws.amazon.com/eks/latest/userguide/managed-node-update-behavior.html
static AWS_EKS_MAX_NODE_DRAIN_TIMEOUT_DURATION: ChronoDuration = ChronoDuration::minutes(15);

impl InfrastructureAction for EKS {
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
        send_progress_on_long_task(self, Action::Create, || {
            create_eks_cluster(
                infra_ctx,
                self,
                infra_ctx.cloud_provider(),
                infra_ctx.dns_provider(),
                &self.s3,
                self.long_id,
                self.template_directory.as_str(),
                &self.zones,
                &self.nodes_groups,
                &self.options,
                &self.advanced_settings,
                self.qovery_allowed_public_access_cidrs.as_ref(),
            )
        })
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
        send_progress_on_long_task(self, Action::Pause, || {
            pause_eks_cluster(
                infra_ctx,
                self,
                infra_ctx.cloud_provider(),
                infra_ctx.dns_provider(),
                self.template_directory.as_str(),
                &self.zones,
                &self.nodes_groups,
                &self.options,
                &self.advanced_settings,
                self.qovery_allowed_public_access_cidrs.as_ref(),
            )
        })
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
        send_progress_on_long_task(self, Action::Delete, || {
            delete_eks_cluster(
                infra_ctx,
                self,
                infra_ctx.cloud_provider(),
                infra_ctx.dns_provider(),
                &self.s3,
                self.template_directory.as_str(),
                &self.zones,
                &self.nodes_groups,
                &self.options,
                &self.advanced_settings,
                self.qovery_allowed_public_access_cidrs.as_ref(),
            )
        })
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
        upgrade_eks_cluster(self, infra_ctx, kubernetes_upgrade_status)
    }
}

use super::utils::from_terraform_value;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AwsEksQoveryTerraformOutput {
    #[serde(deserialize_with = "from_terraform_value")]
    pub aws_account_id: String,
    #[serde(deserialize_with = "from_terraform_value")]
    pub aws_iam_eks_user_mapper_role_arn: String,
    #[serde(deserialize_with = "from_terraform_value")]
    pub aws_iam_cluster_autoscaler_role_arn: String,
    #[serde(deserialize_with = "from_terraform_value")]
    pub aws_iam_cloudwatch_role_arn: String,
    #[serde(deserialize_with = "from_terraform_value")]
    pub aws_iam_loki_role_arn: String,
    #[serde(deserialize_with = "from_terraform_value")]
    pub aws_s3_loki_bucket_name: String,
    #[serde(deserialize_with = "from_terraform_value")]
    pub loki_storage_config_aws_s3: String,
    #[serde(deserialize_with = "from_terraform_value")]
    pub karpenter_controller_aws_role_arn: String,
    #[serde(deserialize_with = "from_terraform_value")]
    pub cluster_security_group_id: String,
    #[serde(deserialize_with = "from_terraform_value")]
    pub aws_iam_alb_controller_arn: String,
}

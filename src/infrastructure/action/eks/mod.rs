mod cluster_bootstrap;
mod cluster_create;
mod cluster_delete;
mod cluster_pause;
mod cluster_upgrade;
mod custom_vpc;
pub(crate) mod helm_charts;
mod karpenter;
mod nodegroup;
mod sdk;
mod tera_context;
mod utils;

use crate::errors::EngineError;
use crate::events::InfrastructureStep;
use crate::infrastructure::action::InfrastructureAction;
use crate::infrastructure::action::eks::cluster_bootstrap::bootstrap_eks_cluster;
use crate::infrastructure::action::eks::cluster_create::create_eks_cluster;
use crate::infrastructure::action::eks::cluster_delete::delete_eks_cluster;
use crate::infrastructure::action::eks::cluster_pause::pause_eks_cluster;
use crate::infrastructure::action::eks::cluster_upgrade::upgrade_eks_cluster;
use crate::infrastructure::infrastructure_context::InfrastructureContext;
use crate::infrastructure::models::cloud_provider::service::Action;
use crate::infrastructure::models::kubernetes::aws::eks::EKS;
use crate::infrastructure::models::kubernetes::{Kubernetes, KubernetesUpgradeStatus, send_progress_on_long_task};
use chrono::Duration as ChronoDuration;
use serde_derive::{Deserialize, Serialize};

static AWS_EKS_DEFAULT_UPGRADE_TIMEOUT_DURATION: ChronoDuration = ChronoDuration::hours(1);
// https://docs.aws.amazon.com/eks/latest/userguide/managed-node-update-behavior.html
static AWS_EKS_MAX_NODE_DRAIN_TIMEOUT_DURATION: ChronoDuration = ChronoDuration::minutes(15);

impl InfrastructureAction for EKS {
    fn bootstap_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        let logger = mk_logger(infra_ctx.kubernetes(), InfrastructureStep::Create);
        send_progress_on_long_task(self, Action::Create, || bootstrap_eks_cluster(self, infra_ctx, logger))
    }

    fn create_cluster(
        &self,
        infra_ctx: &InfrastructureContext,
        has_been_upgraded: bool,
    ) -> Result<(), Box<EngineError>> {
        let logger = mk_logger(infra_ctx.kubernetes(), InfrastructureStep::Create);
        send_progress_on_long_task(self, Action::Create, || {
            create_eks_cluster(self, infra_ctx, has_been_upgraded, logger)
        })
    }

    fn pause_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        let logger = mk_logger(infra_ctx.kubernetes(), InfrastructureStep::Pause);
        send_progress_on_long_task(self, Action::Pause, || pause_eks_cluster(self, infra_ctx, logger))
    }

    fn delete_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        let logger = mk_logger(infra_ctx.kubernetes(), InfrastructureStep::Delete);
        send_progress_on_long_task(self, Action::Delete, || {
            delete_eks_cluster(
                infra_ctx,
                self,
                infra_ctx.cloud_provider(),
                infra_ctx.dns_provider(),
                &self.zones,
                &self.nodes_groups,
                &self.options,
                &self.advanced_settings,
                self.qovery_allowed_public_access_cidrs.as_ref(),
                logger,
            )
        })
    }

    fn upgrade_cluster(
        &self,
        infra_ctx: &InfrastructureContext,
        kubernetes_upgrade_status: KubernetesUpgradeStatus,
    ) -> Result<(), Box<EngineError>> {
        let logger = mk_logger(infra_ctx.kubernetes(), InfrastructureStep::Upgrade);

        send_progress_on_long_task(self, Action::Create, || {
            upgrade_eks_cluster(self, infra_ctx, kubernetes_upgrade_status, logger)
        })
    }

    fn upgrade_node_selector(&self) -> Option<&str> {
        // Exclude fargate nodes from the test in case of karpenter, those will be recreated after helm deploy
        match self.is_karpenter_enabled() {
            true => Some("eks.amazonaws.com/compute-type!=fargate"),
            false => None,
        }
    }
}

use super::utils::{from_terraform_value, mk_logger};

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
    #[serde(deserialize_with = "from_terraform_value")]
    pub aws_iam_eks_prometheus_role_arn: String,
    #[serde(deserialize_with = "from_terraform_value")]
    pub aws_s3_prometheus_bucket_name: String,
    #[serde(deserialize_with = "from_terraform_value")]
    pub kubeconfig: String,
    #[serde(deserialize_with = "from_terraform_value")]
    pub cluster_name: String,
    #[serde(deserialize_with = "from_terraform_value")]
    pub cluster_arn: String,
    #[serde(deserialize_with = "from_terraform_value")]
    pub cluster_id: String,
    #[serde(deserialize_with = "from_terraform_value")]
    pub cluster_oidc_issuer: String,
    #[serde(deserialize_with = "from_terraform_value")]
    pub cluster_vpc_id: String,
}

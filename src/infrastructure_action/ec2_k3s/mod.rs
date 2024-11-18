use crate::cloud_provider::aws::kubernetes::ec2::EC2;
use crate::cloud_provider::kubernetes::{Kubernetes, KubernetesUpgradeStatus};
use crate::engine::InfrastructureContext;
use crate::errors::EngineError;
use crate::events::InfrastructureStep;
use crate::events::Stage::Infrastructure;
use crate::infrastructure_action::{InfraLogger, InfraLoggerImpl, InfrastructureAction};
use serde_derive::{Deserialize, Serialize};

use super::utils::from_terraform_value;

impl InfrastructureAction for EC2 {
    fn create_cluster(
        &self,
        _infra_ctx: &InfrastructureContext,
        _has_been_upgraded: bool,
    ) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Create));
        let logger = InfraLoggerImpl {
            event_details: event_details.clone(),
            logger: self.logger().clone_dyn(),
        };
        logger.warn("Creating a EC2 instance is not supported yet. Skipping this step.");
        Ok(())
    }

    fn pause_cluster(&self, _infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Pause));
        let logger = InfraLoggerImpl {
            event_details: event_details.clone(),
            logger: self.logger().clone_dyn(),
        };
        logger.warn("Pausing a EC2 instance is not supported yet. Skipping this step.");
        Ok(())
    }

    fn delete_cluster(&self, _infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::Delete));
        let logger = InfraLoggerImpl {
            event_details: event_details.clone(),
            logger: self.logger().clone_dyn(),
        };
        logger.warn("Deleting a EC2 instance is not supported yet. Skipping this step.");
        Ok(())
    }

    fn upgrade_cluster(
        &self,
        _infra_ctx: &InfrastructureContext,
        _kubernetes_upgrade_status: KubernetesUpgradeStatus,
    ) -> Result<(), Box<EngineError>> {
        Ok(())
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

use crate::errors::EngineError;
use crate::events::EventDetails;
use async_trait::async_trait;
use aws_sdk_ec2::error::SdkError;
use aws_sdk_ec2::operation::describe_instances::{DescribeInstancesError, DescribeInstancesOutput};
use aws_sdk_ec2::operation::describe_volumes::{DescribeVolumesError, DescribeVolumesOutput};
use aws_sdk_ec2::operation::detach_volume::{DetachVolumeError, DetachVolumeOutput};
use aws_sdk_ec2::types::{Filter, VolumeState};
use aws_types::SdkConfig;

#[async_trait]
pub trait QoveryAwsSdkConfigEc2 {
    async fn get_volume_by_instance_id(
        &self,
        instance_id: String,
    ) -> Result<DescribeVolumesOutput, SdkError<DescribeVolumesError>>;
    async fn detach_instance_volume(
        &self,
        volume_id: String,
    ) -> Result<DetachVolumeOutput, SdkError<DetachVolumeError>>;
    async fn _get_instance_by_id(
        &self,
        instance_id: String,
    ) -> Result<DescribeInstancesOutput, SdkError<DescribeInstancesError>>;
    async fn detach_ec2_volumes(&self, instance_id: &str, event_details: &EventDetails)
        -> Result<(), Box<EngineError>>;
}

#[async_trait]
impl QoveryAwsSdkConfigEc2 for SdkConfig {
    async fn get_volume_by_instance_id(
        &self,
        instance_id: String,
    ) -> Result<DescribeVolumesOutput, SdkError<DescribeVolumesError>> {
        let client = aws_sdk_ec2::Client::new(self);
        client
            .describe_volumes()
            .filters(
                Filter::builder()
                    .name("tag:ClusterId".to_string())
                    .values(instance_id.to_string())
                    .build(),
            )
            .send()
            .await
    }
    async fn detach_instance_volume(
        &self,
        volume_id: String,
    ) -> Result<DetachVolumeOutput, SdkError<DetachVolumeError>> {
        let client = aws_sdk_ec2::Client::new(self);
        client.detach_volume().volume_id(volume_id).send().await
    }
    /// instance isn't used ATM but will be useful when we'll need to implement ec2 pause.
    async fn _get_instance_by_id(
        &self,
        instance_id: String,
    ) -> Result<DescribeInstancesOutput, SdkError<DescribeInstancesError>> {
        let client = aws_sdk_ec2::Client::new(self);
        client
            .describe_instances()
            .filters(
                Filter::builder()
                    .name("tag:ClusterId".to_string())
                    .values(instance_id.to_string())
                    .build(),
            )
            .send()
            .await
    }
    async fn detach_ec2_volumes(
        &self,
        instance_id: &str,
        event_details: &EventDetails,
    ) -> Result<(), Box<EngineError>> {
        let result = match self.get_volume_by_instance_id(instance_id.to_string()).await {
            Ok(result) => result,
            Err(e) => {
                return Err(Box::new(EngineError::new_aws_sdk_cannot_list_ec2_volumes(
                    event_details.clone(),
                    e,
                    Some(instance_id),
                )));
            }
        };

        for volume in result.volumes.unwrap_or_default() {
            if let (Some(id), attachments, Some(state)) = (volume.volume_id(), volume.attachments(), volume.state()) {
                let mut skip_root_volume = false;
                for attachment in attachments {
                    if let Some(device) = attachment.device() {
                        if device.to_string().contains("/dev/xvda") || state != &VolumeState::InUse {
                            skip_root_volume = true;
                        }
                    }
                }
                if skip_root_volume {
                    continue;
                }

                if let Err(e) = self.detach_instance_volume(id.to_string()).await {
                    return Err(Box::new(EngineError::new_aws_sdk_cannot_detach_ec2_volumes(
                        event_details.clone(),
                        e,
                        instance_id,
                        id,
                    )));
                }
            }
        }

        Ok(())
    }
}

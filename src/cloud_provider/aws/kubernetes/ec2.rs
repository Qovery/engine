use crate::cloud_provider::aws::kubernetes;
use crate::cloud_provider::aws::kubernetes::node::AwsInstancesType;
use crate::cloud_provider::aws::kubernetes::Options;
use crate::cloud_provider::aws::models::QoveryAwsSdkConfigEc2;
use crate::cloud_provider::aws::regions::{AwsRegion, AwsZones};
use crate::cloud_provider::io::ClusterAdvancedSettings;
use crate::cloud_provider::kubernetes::{
    event_details, send_progress_on_long_task, InstanceType, Kind, Kubernetes, KubernetesUpgradeStatus,
    KubernetesVersion,
};
use crate::cloud_provider::models::{InstanceEc2, NodeGroups};
use crate::cloud_provider::service::Action;
use crate::cloud_provider::utilities::print_action;
use crate::cloud_provider::CloudProvider;
use crate::dns_provider::DnsProvider;
use crate::errors::EngineError;
use crate::events::{EngineEvent, EventDetails, InfrastructureStep, Stage};
use crate::io_models::context::Context;
use crate::logger::Logger;
use crate::object_storage::s3::S3;
use crate::object_storage::ObjectStorage;
use async_trait::async_trait;
use aws_sdk_ec2::model::{Filter, VolumeState};
use aws_sdk_ec2::types::SdkError;
use aws_types::SdkConfig;
use function_name::named;
use std::borrow::Borrow;
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

/// EC2 kubernetes provider allowing to deploy a cluster on single EC2 node.
pub struct EC2 {
    context: Context,
    id: String,
    long_id: Uuid,
    name: String,
    version: KubernetesVersion,
    region: AwsRegion,
    zones: Vec<AwsZones>,
    cloud_provider: Arc<Box<dyn CloudProvider>>,
    dns_provider: Arc<Box<dyn DnsProvider>>,
    s3: S3,
    template_directory: String,
    options: Options,
    instance: InstanceEc2,
    logger: Box<dyn Logger>,
    advanced_settings: ClusterAdvancedSettings,
}

impl EC2 {
    pub fn new(
        context: Context,
        id: &str,
        long_id: Uuid,
        name: &str,
        version: KubernetesVersion,
        region: AwsRegion,
        zones: Vec<String>,
        cloud_provider: Arc<Box<dyn CloudProvider>>,
        dns_provider: Arc<Box<dyn DnsProvider>>,
        options: Options,
        instance: InstanceEc2,
        logger: Box<dyn Logger>,
        advanced_settings: ClusterAdvancedSettings,
    ) -> Result<Self, Box<EngineError>> {
        let event_details = event_details(&**cloud_provider, long_id, name.to_string(), &context);
        let template_directory = format!("{}/aws-ec2/bootstrap", context.lib_root_dir());

        let aws_zones = kubernetes::aws_zones(zones, &region, &event_details)?;
        advanced_settings.validate(event_details.clone())?;
        let s3 = kubernetes::s3(&context, &region, &**cloud_provider, advanced_settings.pleco_resources_ttl);
        match AwsInstancesType::from_str(instance.instance_type.as_str()) {
            Err(e) => {
                let err = EngineError::new_unsupported_instance_type(event_details, instance.instance_type.as_str(), e);
                logger.log(EngineEvent::Error(err.clone(), None));

                return Err(Box::new(err));
            }
            Ok(instance_type) => {
                if !EC2::is_instance_allowed(instance_type.clone()) {
                    let err = EngineError::new_not_allowed_instance_type(event_details, instance_type.as_str());
                    return Err(Box::new(err));
                }
            }
        }

        // copy listeners from CloudProvider
        Ok(EC2 {
            context,
            id: id.to_string(),
            long_id,
            name: name.to_string(),
            version,
            region,
            zones: aws_zones,
            cloud_provider,
            dns_provider,
            s3,
            options,
            instance,
            template_directory,
            logger,
            advanced_settings,
        })
    }

    fn cloud_provider_name(&self) -> &str {
        "aws"
    }

    fn struct_name(&self) -> &str {
        "kubernetes"
    }

    fn node_group_from_instance_type(&self) -> NodeGroups {
        NodeGroups::new(
            "instance".to_string(),
            1,
            1,
            self.instance.instance_type.clone(),
            self.instance.disk_size_in_gib,
        )
        .expect("wrong instance type for EC2") // using expect here as it has already been validated during instantiation
    }

    pub fn is_instance_allowed(instance_type: AwsInstancesType) -> bool {
        instance_type.is_instance_allowed()
    }
}

impl Kubernetes for EC2 {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::Ec2
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn long_id(&self) -> &Uuid {
        &self.long_id
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn version(&self) -> KubernetesVersion {
        self.version.clone()
    }

    fn region(&self) -> &str {
        self.region.to_aws_format()
    }

    fn zone(&self) -> &str {
        ""
    }

    fn aws_zones(&self) -> Option<Vec<AwsZones>> {
        Some(self.zones.clone())
    }

    fn cloud_provider(&self) -> &dyn CloudProvider {
        (*self.cloud_provider).borrow()
    }

    fn dns_provider(&self) -> &dyn DnsProvider {
        (*self.dns_provider).borrow()
    }

    fn logger(&self) -> &dyn Logger {
        self.logger.borrow()
    }

    fn config_file_store(&self) -> &dyn ObjectStorage {
        &self.s3
    }

    fn is_valid(&self) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn is_network_managed_by_user(&self) -> bool {
        false
    }

    #[named]
    fn on_create(&self) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::Create));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Create, || {
            kubernetes::create(
                self,
                self.long_id,
                self.template_directory.as_str(),
                &self.zones,
                &[self.node_group_from_instance_type()],
                &self.options,
            )
        })
    }

    #[named]
    fn on_create_error(&self) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::Create));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Create, || kubernetes::create_error(self))
    }

    fn upgrade_with_status(&self, _kubernetes_upgrade_status: KubernetesUpgradeStatus) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    #[named]
    fn on_upgrade(&self) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::Upgrade));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Create, || self.upgrade())
    }

    #[named]
    fn on_upgrade_error(&self) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::Upgrade));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Create, || kubernetes::upgrade_error(self))
    }

    #[named]
    fn on_pause(&self) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::Pause));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Pause, || {
            kubernetes::pause(self, self.template_directory.as_str(), &self.zones, &[], &self.options)
        })
    }

    #[named]
    fn on_pause_error(&self) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::Pause));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Pause, || kubernetes::pause_error(self))
    }

    #[named]
    fn on_delete(&self) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::Delete));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Delete, || {
            kubernetes::delete(
                self,
                self.template_directory.as_str(),
                &self.zones,
                &[self.node_group_from_instance_type()],
                &self.options,
            )
        })
    }

    #[named]
    fn on_delete_error(&self) -> Result<(), Box<EngineError>> {
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::Delete));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );
        send_progress_on_long_task(self, Action::Delete, || kubernetes::delete_error(self))
    }

    fn advanced_settings(&self) -> &ClusterAdvancedSettings {
        &self.advanced_settings
    }
}

#[async_trait]
impl QoveryAwsSdkConfigEc2 for SdkConfig {
    async fn get_volume_by_instance_id(
        &self,
        instance_id: String,
    ) -> Result<aws_sdk_ec2::output::DescribeVolumesOutput, SdkError<aws_sdk_ec2::error::DescribeVolumesError>> {
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
    ) -> Result<aws_sdk_ec2::output::DetachVolumeOutput, SdkError<aws_sdk_ec2::error::DetachVolumeError>> {
        let client = aws_sdk_ec2::Client::new(self);
        client.detach_volume().volume_id(volume_id).send().await
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
                )))
            }
        };

        if let Some(volumes) = result.volumes() {
            for volume in volumes {
                if let (Some(id), Some(attachments), Some(state)) =
                    (volume.volume_id(), volume.attachments(), volume.state())
                {
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
        }

        Ok(())
    }
}

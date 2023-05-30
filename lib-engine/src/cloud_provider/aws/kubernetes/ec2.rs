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
use crate::cloud_provider::models::{CpuArchitecture, InstanceEc2, NodeGroups, NodeGroupsWithDesiredState};
use crate::cloud_provider::service::Action;
use crate::cloud_provider::utilities::{print_action, wait_until_port_is_open, TcpCheckSource};
use crate::cloud_provider::vault::{ClusterSecrets, ClusterSecretsAws};
use crate::cloud_provider::CloudProvider;
use crate::cmd::terraform::{terraform_init_validate_plan_apply, TerraformError};
use crate::dns_provider::DnsProvider;
use crate::errors::EngineError;
use crate::events::{EngineEvent, EventDetails, EventMessage, InfrastructureStep, Stage};
use crate::io_models::context::Context;
use crate::logger::Logger;
use crate::object_storage::s3::S3;
use crate::object_storage::ObjectStorage;
use crate::secret_manager::vault::QVaultClient;
use async_trait::async_trait;
use aws_sdk_ec2::model::{Filter, VolumeState};
use aws_sdk_ec2::types::SdkError;
use aws_types::SdkConfig;
use function_name::named;
use retry::delay::Fixed;
use retry::Error::Operation;
use retry::{Error, OperationResult};
use std::borrow::Borrow;
use std::io::Read;
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

use super::ec2_helm_charts::get_aws_ec2_qovery_terraform_config;

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
            self.instance.instance_architecture,
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

    fn cpu_architectures(&self) -> Vec<CpuArchitecture> {
        vec![self.instance.instance_architecture]
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
        let event_details = self.get_event_details(Stage::Infrastructure(InfrastructureStep::Upgrade));

        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Start preparing EC2 node upgrade process".to_string()),
        ));

        let temp_dir = self.get_temp_dir(event_details.clone())?;

        // generate terraform files and copy them into temp dir
        let context = kubernetes::tera_context(
            self,
            &self.zones,
            &[NodeGroupsWithDesiredState::new_from_node_groups(
                &self.node_group_from_instance_type(),
                1,
                false,
            )],
            &self.options,
        )?;

        if let Err(e) = crate::template::generate_and_copy_all_files_into_dir(
            self.template_directory.as_str(),
            temp_dir.as_str(),
            context,
        ) {
            return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                event_details,
                self.template_directory.to_string(),
                temp_dir,
                e,
            )));
        }

        // copy lib/common/bootstrap/charts directory (and sub directory) into the lib/aws/bootstrap/common/charts directory.
        // this is due to the required dependencies of lib/aws/bootstrap/*.tf files
        let common_charts_temp_dir = format!("{}/common/charts", temp_dir.as_str());
        let common_bootstrap_charts = format!("{}/common/bootstrap/charts", self.context.lib_root_dir());
        if let Err(e) =
            crate::template::copy_non_template_files(common_bootstrap_charts.as_str(), common_charts_temp_dir.as_str())
        {
            return Err(Box::new(EngineError::new_cannot_copy_files_from_one_directory_to_another(
                event_details,
                common_bootstrap_charts,
                common_charts_temp_dir,
                e,
            )));
        }

        terraform_init_validate_plan_apply(
            temp_dir.as_str(),
            self.context.is_dry_run_deploy(),
            self.cloud_provider().credentials_environment_variables().as_slice(),
        )
        .map_err(|e| EngineError::new_terraform_error(event_details.clone(), e))?;

        // update Vault with new cluster information
        self.logger().log(EngineEvent::Info(
            event_details.clone(),
            EventMessage::new_from_safe("Ensuring the upgrade has successfuly been performed...".to_string()),
        ));

        let cluster_secrets = ClusterSecrets::new_aws_eks(ClusterSecretsAws::new(
            self.cloud_provider().access_key_id(),
            self.region().to_string(),
            self.cloud_provider().secret_access_key(),
            None,
            None,
            self.kind(),
            self.cluster_name(),
            self.long_id().to_string(),
            self.options.grafana_admin_user.clone(),
            self.options.grafana_admin_password.clone(),
            self.cloud_provider().organization_long_id().to_string(),
            self.context().is_test_cluster(),
        ));

        let qovery_terraform_config_file = format!("{}/qovery-tf-config.json", &temp_dir);
        if let Err(e) =
            self.update_vault_config(event_details.clone(), qovery_terraform_config_file, cluster_secrets, None)
        {
            self.logger().log(EngineEvent::Warning(
                event_details.clone(),
                EventMessage::new(
                    "Wasn't able to update Vault information for this EC2 instance".to_string(),
                    Some(e.to_string()),
                ),
            ));
        };

        self.logger().log(EngineEvent::Info(
            event_details,
            EventMessage::new_from_safe("Kubernetes nodes have been successfully upgraded".to_string()),
        ));

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

    /// Update the vault with the new cluster information
    /// !!! Can only work has been just updated with Terraform data !!!
    fn update_vault_config(
        &self,
        event_details: EventDetails,
        qovery_terraform_config_file: String,
        cluster_secrets: ClusterSecrets,
        _kubeconfig_file_path: Option<String>,
    ) -> Result<(), Box<EngineError>> {
        // read config generated after terraform infra bootstrap/update
        let qovery_terraform_config = get_aws_ec2_qovery_terraform_config(qovery_terraform_config_file.as_str())
            .map_err(|e| EngineError::new_terraform_error(event_details.clone(), e))?;

        // send cluster info to vault if info mismatch
        // create vault connection (Vault connectivity should not be on the critical deployment path,
        // if it temporarily fails, just ignore it, data will be pushed on the next sync)
        let vault_conn = match QVaultClient::new(event_details.clone()) {
            Ok(x) => Some(x),
            Err(_) => None,
        };
        if let Some(vault) = &vault_conn {
            let mut cluster_secrets_update = cluster_secrets.clone();
            cluster_secrets_update.set_k8s_cluster_endpoint(qovery_terraform_config.aws_ec2_public_hostname.clone());
            // update info without taking care of the kubeconfig because we don't have it yet
            let _ = cluster_secrets_update.create_or_update_secret(vault, true, event_details.clone())?;
        };

        let port = match qovery_terraform_config.kubernetes_port_to_u16() {
            Ok(p) => p,
            Err(e) => {
                return Err(Box::new(EngineError::new_terraform_error(
                    event_details,
                    TerraformError::ConfigFileInvalidContent {
                        path: qovery_terraform_config_file,
                        raw_message: e,
                    },
                )))
            }
        };

        // wait for k3s port to be open
        // retry for 10 min, a reboot will occur after 5 min if nothing happens (see EC2 Terraform user config)
        wait_until_port_is_open(
            &TcpCheckSource::DnsName(qovery_terraform_config.aws_ec2_public_hostname.as_str()),
            port,
            600,
            self.logger(),
            event_details.clone(),
        )
        .map_err(|_| EngineError::new_k8s_cannot_reach_api(event_details.clone()))?;

        // during an instance replacement, the EC2 host dns will change and will require the kubeconfig to be updated
        // we need to ensure the kubeconfig is the correct one by checking the current instance dns in the kubeconfig
        let result = retry::retry(Fixed::from_millis(5 * 1000).take(120), || {
            // force s3 kubeconfig retrieve
            if let Err(e) = self.delete_local_kubeconfig_object_storage_folder() {
                return OperationResult::Err(e);
            };
            let (current_kubeconfig_path, mut kubeconfig_file) = match self.get_kubeconfig_file() {
                Ok(x) => x,
                Err(e) => return OperationResult::Retry(e),
            };

            // ensure the kubeconfig content address match with the current instance dns
            let mut buffer = String::new();
            let _ = kubeconfig_file.read_to_string(&mut buffer);
            match buffer.contains(&qovery_terraform_config.aws_ec2_public_hostname) {
                true => {
                    self.logger().log(EngineEvent::Info(
                        event_details.clone(),
                        EventMessage::new_from_safe(format!(
                            "kubeconfig stored on s3 do correspond with the actual host {}",
                            &qovery_terraform_config.aws_ec2_public_hostname
                        )),
                    ));
                    OperationResult::Ok(current_kubeconfig_path)
                }
                false => {
                    self.logger().log(EngineEvent::Warning(
                            event_details.clone(),
                            EventMessage::new_from_safe(format!(
                                "kubeconfig stored on s3 do not yet correspond with the actual host {}, retrying in 5 sec...",
                                &qovery_terraform_config.aws_ec2_public_hostname
                            )),
                        ));
                    OperationResult::Retry(Box::new(EngineError::new_kubeconfig_file_do_not_match_the_current_cluster(
                        event_details.clone(),
                    )))
                }
            }
        });

        match result {
            Ok(x) => {
                // update to store the correct kubeconfig
                if let Some(vault) = &vault_conn {
                    let new_kubeconfig_b64 = base64::encode(x);
                    let mut cluster_secrets_update = cluster_secrets;
                    cluster_secrets_update.set_k8s_cluster_endpoint(qovery_terraform_config.aws_ec2_public_hostname);
                    cluster_secrets_update.set_kubeconfig_b64(new_kubeconfig_b64);
                    // update info without taking care of the kubeconfig because we don't have it yet
                    let _ = cluster_secrets_update.create_or_update_secret(vault, true, event_details);
                };
                Ok(())
            }
            Err(Operation { error, .. }) => Err(error),
            Err(Error::Internal(_)) => Err(Box::new(
                EngineError::new_kubeconfig_file_do_not_match_the_current_cluster(event_details),
            )),
        }
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
    /// instance isn't used ATM but will be useful when we'll need to implement ec2 pause.
    async fn _get_instance_by_id(
        &self,
        instance_id: String,
    ) -> Result<aws_sdk_ec2::output::DescribeInstancesOutput, SdkError<aws_sdk_ec2::error::DescribeInstancesError>>
    {
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

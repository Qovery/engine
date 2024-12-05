use crate::cloud_provider::aws::kubernetes;
use crate::cloud_provider::aws::kubernetes::node::AwsInstancesType;
use crate::cloud_provider::aws::kubernetes::Options;
use crate::cloud_provider::aws::regions::{AwsRegion, AwsZone};
use crate::cloud_provider::io::ClusterAdvancedSettings;
use crate::cloud_provider::kubernetes::{event_details, InstanceType, Kind, Kubernetes, KubernetesVersion};
use crate::cloud_provider::models::{CpuArchitecture, InstanceEc2, NodeGroups};
use crate::cloud_provider::vault::ClusterSecrets;
use crate::cloud_provider::CloudProvider;
use crate::errors::{CommandError, EngineError};
use crate::events::{EngineEvent, EventDetails};
use crate::infrastructure_action::InfrastructureAction;
use crate::io_models::context::Context;
use crate::io_models::engine_request::{ChartValuesOverrideName, ChartValuesOverrideValues};
use crate::logger::Logger;
use crate::models::ToCloudProviderFormat;
use crate::object_storage::s3::S3;
use crate::secret_manager::vault::QVaultClient;
use crate::utilities::to_short_id;
use base64::engine::general_purpose;
use base64::Engine;
use std::borrow::Borrow;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use uuid::Uuid;

/// EC2 kubernetes provider allowing to deploy a cluster on single EC2 node.
pub struct EC2 {
    pub context: Context,
    pub id: String,
    pub long_id: Uuid,
    pub name: String,
    pub version: KubernetesVersion,
    pub region: AwsRegion,
    pub zones: Vec<AwsZone>,
    pub s3: S3,
    pub template_directory: String,
    pub options: Options,
    pub instance: InstanceEc2,
    pub logger: Box<dyn Logger>,
    pub advanced_settings: ClusterAdvancedSettings,
    pub customer_helm_charts_override: Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>>,
    pub _kubeconfig: Option<String>,
    pub temp_dir: PathBuf,
}

impl EC2 {
    pub fn new(
        context: Context,
        long_id: Uuid,
        name: &str,
        version: KubernetesVersion,
        region: AwsRegion,
        zones: Vec<String>,
        cloud_provider: &dyn CloudProvider,
        options: Options,
        instance: InstanceEc2,
        logger: Box<dyn Logger>,
        advanced_settings: ClusterAdvancedSettings,
        customer_helm_charts_override: Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>>,
        kubeconfig: Option<String>,
        temp_dir: PathBuf,
    ) -> Result<Self, Box<EngineError>> {
        let event_details = event_details(cloud_provider, long_id, name.to_string(), &context);
        let template_directory = format!("{}/aws-ec2/bootstrap", context.lib_root_dir());

        let aws_zones = kubernetes::aws_zones(zones, &region, &event_details)?;
        advanced_settings.validate(event_details.clone())?;
        let s3 = mk_s3(&region, cloud_provider);
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
        let cluster = EC2 {
            context,
            id: to_short_id(&long_id),
            long_id,
            name: name.to_string(),
            version,
            region,
            zones: aws_zones,
            s3,
            options,
            instance,
            template_directory,
            logger,
            advanced_settings,
            customer_helm_charts_override,
            _kubeconfig: kubeconfig,
            temp_dir,
        };

        Ok(cluster)
    }

    pub fn struct_name(&self) -> &str {
        "kubernetes"
    }

    pub fn node_group_from_instance_type(&self) -> NodeGroups {
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

    fn short_id(&self) -> &str {
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
        self.region.to_cloud_provider_format()
    }

    fn zones(&self) -> Option<Vec<&str>> {
        Some(self.zones.iter().map(|z| z.to_cloud_provider_format()).collect())
    }

    fn logger(&self) -> &dyn Logger {
        self.logger.borrow()
    }

    fn is_network_managed_by_user(&self) -> bool {
        false
    }

    fn cpu_architectures(&self) -> Vec<CpuArchitecture> {
        vec![self.instance.instance_architecture]
    }

    fn temp_dir(&self) -> &Path {
        &self.temp_dir
    }

    /// Update the vault with the new cluster information
    /// !!! Can only work has been just updated with Terraform data !!!
    fn update_vault_config(
        &self,
        event_details: EventDetails,
        mut cluster_secrets: ClusterSecrets,
        kubeconfig_file_path: Option<&Path>,
    ) -> Result<(), Box<EngineError>> {
        // send cluster info to vault if info mismatch
        // create vault connection (Vault connectivity should not be on the critical deployment path,
        // if it temporarily fails, just ignore it, data will be pushed on the next sync)
        let Ok(vault_conn) = QVaultClient::new(event_details.clone()) else {
            return Ok(());
        };

        if let Some(x) = kubeconfig_file_path {
            // encode base64 kubeconfig
            let kubeconfig = fs::read_to_string(x)
                .map_err(|e| {
                    EngineError::new_cannot_retrieve_cluster_config_file(
                        event_details.clone(),
                        CommandError::new_from_safe_message(format!(
                            "Cannot read kubeconfig file {}: {e}",
                            x.to_str().unwrap_or_default()
                        )),
                    )
                })
                .expect("kubeconfig was not found while it should be present");
            let kubeconfig_b64 = general_purpose::STANDARD.encode(kubeconfig);
            cluster_secrets.set_kubeconfig_b64(kubeconfig_b64);
        }

        cluster_secrets.create_or_update_secret(&vault_conn, true, event_details)?;

        Ok(())
    }

    fn advanced_settings(&self) -> &ClusterAdvancedSettings {
        &self.advanced_settings
    }

    fn loadbalancer_l4_annotations(&self, _cloud_provider_lb_name: Option<&str>) -> Vec<(String, String)> {
        vec![(
            "service.beta.kubernetes.io/aws-load-balancer-type".to_string(),
            "nlb".to_string(),
        )]
    }

    fn as_infra_actions(&self) -> &dyn InfrastructureAction {
        self
    }
}

pub fn mk_s3(region: &AwsRegion, cloud_provider: &dyn CloudProvider) -> S3 {
    S3::new(
        "s3-temp-id".to_string(),
        "default-s3".to_string(),
        cloud_provider.access_key_id(),
        cloud_provider.secret_access_key(),
        region.clone(),
    )
}

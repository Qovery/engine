use crate::environment::models::ToCloudProviderFormat;
use crate::environment::models::azure::Credentials;
use crate::errors::EngineError;
use crate::events::InfrastructureStep;
use crate::events::Stage::Infrastructure;
use crate::infrastructure::action::InfrastructureAction;
use crate::infrastructure::action::kubeconfig_helper::write_kubeconfig_on_disk;
use crate::infrastructure::models::cloud_provider::CloudProvider;
use crate::infrastructure::models::cloud_provider::azure::locations::AzureLocation;
use crate::infrastructure::models::cloud_provider::io::ClusterAdvancedSettings;
use crate::infrastructure::models::kubernetes::azure::AksOptions;
use crate::infrastructure::models::kubernetes::azure::node_group::AzureNodeGroups;
use crate::infrastructure::models::kubernetes::{Kind, Kubernetes, KubernetesVersion, event_details};
use crate::infrastructure::models::object_storage::azure_object_storage::AzureOS;
use crate::io_models::context::Context;
use crate::io_models::engine_request::{ChartValuesOverrideName, ChartValuesOverrideValues};
use crate::io_models::models::CpuArchitecture;
use crate::logger::Logger;
use crate::services::azure::blob_storage_service::BlobStorageService;
use crate::utilities::to_short_id;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

pub struct AKS {
    pub context: Context,
    pub id: String,
    pub long_id: Uuid,
    pub name: String,
    pub version: KubernetesVersion,
    pub location: AzureLocation,
    pub created_at: DateTime<Utc>,
    pub blob_storage: AzureOS,
    pub template_directory: PathBuf,
    pub options: AksOptions,
    pub logger: Box<dyn Logger>,
    pub advanced_settings: ClusterAdvancedSettings,
    pub customer_helm_charts_override: Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>>,
    pub kubeconfig: Option<String>,
    pub temp_dir: PathBuf,
    pub qovery_allowed_public_access_cidrs: Option<Vec<String>>,
    pub credentials: Credentials,
    pub node_groups: AzureNodeGroups,
}

impl AKS {
    pub fn new(
        context: Context,
        long_id: Uuid,
        name: &str,
        version: KubernetesVersion,
        location: AzureLocation,
        cloud_provider: &dyn CloudProvider,
        created_at: DateTime<Utc>,
        options: AksOptions,
        node_groups: AzureNodeGroups,
        logger: Box<dyn Logger>,
        advanced_settings: ClusterAdvancedSettings,
        customer_helm_charts_override: Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>>,
        kubeconfig: Option<String>,
        temp_dir: PathBuf,
        qovery_allowed_public_access_cidrs: Option<Vec<String>>,
    ) -> Result<Self, Box<EngineError>> {
        let event_details = event_details(cloud_provider, long_id, name.to_string(), &context);
        let template_directory = PathBuf::from(format!("{}/azure/bootstrap", context.lib_root_dir()));

        advanced_settings.validate(event_details.clone())?;

        let credentials = cloud_provider
            .downcast_ref()
            .as_azure()
            .ok_or_else(|| Box::new(EngineError::new_bad_cast(event_details.clone(), "Cloudprovider is not Azure")))?
            .credentials
            .clone();

        let short_id = to_short_id(&long_id);

        // Blob storage
        // TODO(benjaminch): Storage location should be deduced from the AKS location
        let blob_storage_service = BlobStorageService::new();
        let blob_storage = AzureOS::new(short_id.as_str(), long_id, name, Arc::from(blob_storage_service));

        let cluster = AKS {
            context,
            id: short_id,
            long_id,
            name: name.to_string(),
            version,
            location,
            created_at,
            blob_storage,
            template_directory,
            options,
            node_groups,
            logger,
            advanced_settings,
            customer_helm_charts_override,
            kubeconfig,
            temp_dir,
            qovery_allowed_public_access_cidrs,
            credentials,
        };

        if let Some(kubeconfig) = &cluster.kubeconfig {
            write_kubeconfig_on_disk(
                &cluster.kubeconfig_local_file_path(),
                kubeconfig,
                cluster.get_event_details(Infrastructure(InfrastructureStep::LoadConfiguration)),
            )?;
        }

        Ok(cluster)
    }

    pub fn kubeconfig_bucket_name(&self) -> String {
        format!("qovery-kubeconfigs-{}", self.short_id())
    }

    pub fn logs_bucket_name(&self) -> String {
        format!("qovery-logs-{}", self.id)
    }
}

impl Kubernetes for AKS {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::Aks
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
        self.location.to_cloud_provider_format()
    }

    fn zones(&self) -> Option<Vec<&str>> {
        None
    }

    fn logger(&self) -> &dyn Logger {
        self.logger.as_ref()
    }

    fn is_network_managed_by_user(&self) -> bool {
        // TODO(benjaminch): implement this
        //matches!(self.options.vpc_mode, VpcMode::UserNetworkConfig { .. })
        false
    }

    fn cpu_architectures(&self) -> Vec<CpuArchitecture> {
        self.node_groups
            .get_all_node_groups()
            .iter()
            .map(|node| node.instance_architecture)
            .collect()
    }

    fn temp_dir(&self) -> &Path {
        &self.temp_dir
    }

    fn advanced_settings(&self) -> &ClusterAdvancedSettings {
        &self.advanced_settings
    }

    fn loadbalancer_l4_annotations(&self, _cloud_provider_lb_name: Option<&str>) -> Vec<(String, String)> {
        Vec::with_capacity(0)
    }

    fn as_infra_actions(&self) -> &dyn InfrastructureAction {
        self
    }
}

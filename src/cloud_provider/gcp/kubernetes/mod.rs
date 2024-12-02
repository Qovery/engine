use crate::cloud_provider::gcp::locations::GcpRegion;
use crate::cloud_provider::io::ClusterAdvancedSettings;
use crate::cloud_provider::kubeconfig_helper::write_kubeconfig_on_disk;
use crate::cloud_provider::kubernetes::{Kind, Kubernetes, KubernetesVersion, ProviderOptions};
use crate::cloud_provider::models::{CpuArchitecture, VpcQoveryNetworkMode};
use crate::cloud_provider::qovery::EngineLocation;
use crate::cmd::command::{ExecutableCommand, QoveryCommand};
use crate::errors::EngineError;
use crate::events::Stage::Infrastructure;
use crate::events::{EventDetails, InfrastructureStep, Transmitter};
use crate::io_models::context::Context;
use crate::io_models::engine_request::{ChartValuesOverrideName, ChartValuesOverrideValues};
use crate::io_models::QoveryIdentifier;
use crate::logger::Logger;

use crate::cloud_provider;
use crate::engine::InfrastructureContext;
use crate::models::gcp::JsonCredentials;
use crate::models::ToCloudProviderFormat;
use crate::object_storage::errors::ObjectStorageError;
use crate::object_storage::google_object_storage::GoogleOS;
use crate::services::gcp::auth_service::GoogleAuthService;
use crate::services::gcp::object_storage_regions::GcpStorageRegion;
use crate::services::gcp::object_storage_service::ObjectStorageService;

use crate::infrastructure_action::InfrastructureAction;
use crate::utilities::to_short_id;
use governor::{Quota, RateLimiter};
use ipnet::IpNet;
use nonzero_ext::nonzero;
use once_cell::sync::Lazy;
use retry::delay::Fixed;
use retry::OperationResult;
use std::borrow::Borrow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use time::Time;
use uuid::Uuid;

// Namespaces which are not deleteable because managed by GKE
// Example error: GKE Warden authz [denied by managed-namespaces-limitation]: the namespace "gke-gmp-system" is managed and the request's verb "delete" is denied
pub static GKE_AUTOPILOT_PROTECTED_K8S_NAMESPACES: Lazy<Vec<&str>> = Lazy::new(|| {
    vec![
        "kube-system",
        "gke-gmp-system",
        "gke-managed-filestorecsi",
        "gmp-public",
        "gke-managed-cim",
    ]
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VpcMode {
    Automatic {
        custom_cluster_ipv4_cidr_block: Option<IpNet>,
        custom_services_ipv4_cidr_block: Option<IpNet>,
    },
    UserNetworkConfig {
        vpc_project_id: Option<String>,
        vpc_name: String,
        subnetwork_name: Option<String>,
        ip_range_pods_name: Option<String>,
        additional_ip_range_pods_names: Option<Vec<String>>,
        ip_range_services_name: Option<String>,
    },
}

impl VpcMode {
    pub fn new_automatic(
        custom_cluster_ipv4_cidr_block: Option<IpNet>,
        custom_services_ipv4_cidr_block: Option<IpNet>,
    ) -> Self {
        VpcMode::Automatic {
            custom_cluster_ipv4_cidr_block,
            custom_services_ipv4_cidr_block,
        }
    }

    pub fn new_user_network_config(
        vpc_project_id: Option<String>,
        vpc_name: String,
        subnetwork_name: Option<String>,
        ip_range_pods_name: Option<String>,
        additional_ip_range_pods_names: Option<Vec<String>>,
        ip_range_services_name: Option<String>,
    ) -> Self {
        VpcMode::UserNetworkConfig {
            vpc_project_id,
            vpc_name,
            subnetwork_name,
            ip_range_pods_name,
            additional_ip_range_pods_names,
            ip_range_services_name,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GkeOptions {
    // Qovery
    pub qovery_api_url: String,
    pub qovery_grpc_url: String,
    pub qovery_engine_url: String,
    pub jwt_token: String,
    pub qovery_ssh_key: String,
    pub user_ssh_keys: Vec<String>,
    pub grafana_admin_user: String,
    pub grafana_admin_password: String,
    pub qovery_engine_location: EngineLocation,

    // GCP
    pub gcp_json_credentials: JsonCredentials,

    // Network
    // VPC
    pub vpc_mode: VpcMode,
    pub vpc_qovery_network_mode: Option<VpcQoveryNetworkMode>,

    // GCP to be checked during integration if needed:
    pub cluster_maintenance_start_time: Time,
    pub cluster_maintenance_end_time: Option<Time>,

    // Other
    pub tls_email_report: String,
}

impl GkeOptions {
    pub fn new(
        qovery_api_url: String,
        qovery_grpc_url: String,
        qovery_engine_url: String,
        jwt_token: String,
        qovery_ssh_key: String,
        user_ssh_keys: Vec<String>,
        grafana_admin_user: String,
        grafana_admin_password: String,
        qovery_engine_location: EngineLocation,
        gcp_json_credentials: JsonCredentials,
        vpc_mode: VpcMode,
        vpc_qovery_network_mode: Option<VpcQoveryNetworkMode>,
        tls_email_report: String,
        cluster_maintenance_start_time: Time,
        cluster_maintenance_end_time: Option<Time>,
    ) -> Self {
        GkeOptions {
            qovery_api_url,
            qovery_grpc_url,
            qovery_engine_url,
            jwt_token,
            qovery_ssh_key,
            user_ssh_keys,
            grafana_admin_user,
            grafana_admin_password,
            qovery_engine_location,
            gcp_json_credentials,
            vpc_mode,
            vpc_qovery_network_mode,
            tls_email_report,
            cluster_maintenance_start_time,
            cluster_maintenance_end_time,
        }
    }
}

impl ProviderOptions for GkeOptions {}

pub struct Gke {
    pub context: Context,
    pub id: String,
    pub long_id: Uuid,
    pub name: String,
    pub version: KubernetesVersion,
    pub region: GcpRegion,
    pub template_directory: PathBuf,
    pub object_storage: GoogleOS,
    pub options: GkeOptions,
    pub logger: Box<dyn Logger>,
    pub advanced_settings: ClusterAdvancedSettings,
    pub customer_helm_charts_override: Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>>,
    pub kubeconfig: Option<String>,
    pub temp_dir: PathBuf,
}

impl Gke {
    pub fn new(
        context: Context,
        long_id: Uuid,
        name: &str,
        version: KubernetesVersion,
        region: GcpRegion,
        options: GkeOptions,
        logger: Box<dyn Logger>,
        advanced_settings: ClusterAdvancedSettings,
        customer_helm_charts_override: Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>>,
        kubeconfig: Option<String>,
        temp_dir: PathBuf,
    ) -> Result<Self, Box<EngineError>> {
        let event_details = EventDetails::new(
            Some(cloud_provider::Kind::Gcp),
            QoveryIdentifier::new(*context.organization_long_id()),
            QoveryIdentifier::new(*context.cluster_long_id()),
            context.borrow().execution_id().to_string(),
            Infrastructure(InfrastructureStep::LoadConfiguration),
            Transmitter::Kubernetes(long_id, name.to_string()),
        );

        let object_storage_service_client = retry::retry(Fixed::from(Duration::from_secs(20)).take(3), || {
            match ObjectStorageService::new(
                options.gcp_json_credentials.clone(),
                // A rate limiter making sure to keep the QPS under quotas while bucket writes requests
                // Max default quotas are 0.5 RPS
                // more info here https://cloud.google.com/storage/quotas?hl=fr
                Some(Arc::from(RateLimiter::direct(Quota::per_minute(nonzero!(30_u32))))),
                // A rate limiter making sure to keep the QPS under quotas while bucket objects writes requests
                // Max default quotas are 1 RPS
                // more info here https://cloud.google.com/storage/quotas?hl=fr
                Some(Arc::from(RateLimiter::direct(Quota::per_second(nonzero!(1_u32))))),
            ) {
                Ok(client) => OperationResult::Ok(client),
                Err(error) => {
                    let object_storage_error = EngineError::new_object_storage_error(
                        event_details.clone(),
                        ObjectStorageError::CannotInstantiateClient {
                            raw_error_message: error.to_string(),
                        },
                    );
                    // Only retry if the operation timed out (no other way than looking in raw_error content)
                    if error.get_raw_error_message().contains("operation timed out") {
                        OperationResult::Retry(object_storage_error)
                    } else {
                        OperationResult::Err(object_storage_error)
                    }
                }
            }
        })
        .map_err(|error| Box::new(error.error))?;
        let short_id = to_short_id(&long_id);
        let google_object_storage = GoogleOS::new(
            &short_id,
            long_id,
            name,
            &options.gcp_json_credentials.project_id.to_string(),
            GcpStorageRegion::from(region.clone()),
            Arc::new(object_storage_service_client),
        );

        let cluster = Self {
            context: context.clone(),
            id: short_id,
            long_id,
            name: name.to_string(),
            version,
            region,
            template_directory: PathBuf::from(context.lib_root_dir()).join("gcp/bootstrap"),
            object_storage: google_object_storage,
            options,
            logger,
            advanced_settings,
            customer_helm_charts_override,
            kubeconfig,
            temp_dir,
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

    pub fn configure_gcloud_for_cluster(&self, infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        // Configure kubectl to be able to connect to cluster
        // https://cloud.google.com/kubernetes-engine/docs/how-to/cluster-access-for-kubectl#gcloud_1

        if let Err(e) = GoogleAuthService::activate_service_account(self.options.gcp_json_credentials.clone()) {
            error!("Cannot activate service account: {}", e);
            // TODO(ENG-1803): introduce an EngineError for it and handle it properly
        }

        let _ = QoveryCommand::new(
            "gcloud",
            &[
                "container",
                "clusters",
                "get-credentials",
                self.cluster_name().as_str(),
                format!("--region={}", self.region.to_cloud_provider_format()).as_str(),
                format!("--project={}", self.options.gcp_json_credentials.project_id).as_str(),
            ],
            infra_ctx
                .cloud_provider()
                .credentials_environment_variables()
                .as_slice(),
        )
        .exec(); // TODO(ENG-1804): introduce an EngineError for it and handle it properly

        Ok(())
    }
}

impl Kubernetes for Gke {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::Gke
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
        let zones = self.region.zones();
        if !zones.is_empty() {
            return Some(zones.iter().map(|z| z.to_cloud_provider_format()).collect());
        }
        None
    }

    fn logger(&self) -> &dyn Logger {
        self.logger.as_ref()
    }

    fn is_network_managed_by_user(&self) -> bool {
        matches!(self.options.vpc_mode, VpcMode::UserNetworkConfig { .. })
    }

    fn cpu_architectures(&self) -> Vec<CpuArchitecture> {
        // TODO(ENG-1643): GKE integration, add ARM support
        vec![CpuArchitecture::AMD64]
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

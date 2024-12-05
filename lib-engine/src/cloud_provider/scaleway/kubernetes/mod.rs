pub mod node;

use crate::cloud_provider::io::ClusterAdvancedSettings;
use crate::cloud_provider::kubeconfig_helper::write_kubeconfig_on_disk;
use crate::cloud_provider::kubernetes::{self, InstanceType, Kind, Kubernetes, KubernetesVersion, ProviderOptions};
use crate::cloud_provider::models::{CpuArchitecture, NodeGroups};
use crate::cloud_provider::qovery::EngineLocation;
use crate::cloud_provider::scaleway::kubernetes::node::ScwInstancesType;
use crate::cloud_provider::vault::ClusterSecrets;
use crate::cloud_provider::CloudProvider;
use crate::errors::{CommandError, EngineError};
use crate::events::Stage::Infrastructure;
use crate::events::{EngineEvent, EventDetails, InfrastructureStep, Transmitter};
use crate::io_models::context::Context;
use crate::io_models::engine_request::{ChartValuesOverrideName, ChartValuesOverrideValues};
use crate::io_models::QoveryIdentifier;
use crate::logger::Logger;

use crate::infrastructure_action::InfrastructureAction;
use crate::models::domain::ToTerraformString;
use crate::models::scaleway::ScwZone;
use crate::object_storage::scaleway_object_storage::ScalewayOS;
use crate::runtime::block_on;
use crate::secret_manager::vault::QVaultClient;
use crate::utilities::to_short_id;
use base64::engine::general_purpose;
use base64::Engine;
use scaleway_api_rs::models::ScalewayK8sV1Cluster;
use serde::{Deserialize, Serialize};
use std::borrow::Borrow;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use uuid::Uuid;

#[derive(PartialEq, Eq)]
pub enum ScwNodeGroupErrors {
    CloudProviderApiError(CommandError),
    ClusterDoesNotExists(CommandError),
    MultipleClusterFound,
    NoNodePoolFound(CommandError),
    MissingNodePoolInfo(String),
    NodeGroupValidationError(CommandError),
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum KapsuleClusterType {
    #[default]
    Kapsule, // Mutualized control plane
    KapsuleDedicated4,
    KapsuleDedicated8,
    KapsuleDedicated16,
}

impl ToTerraformString for KapsuleClusterType {
    fn to_terraform_format_string(&self) -> String {
        match self {
            KapsuleClusterType::Kapsule => "kapsule".to_string(),
            KapsuleClusterType::KapsuleDedicated4 => "kapsule-dedicated-4".to_string(),
            KapsuleClusterType::KapsuleDedicated8 => "kapsule-dedicated-8".to_string(),
            KapsuleClusterType::KapsuleDedicated16 => "kapsule-dedicated-16".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KapsuleOptions {
    // Qovery
    pub qovery_api_url: String,
    pub qovery_grpc_url: String,
    #[serde(default)]
    pub qovery_engine_url: String,
    pub jwt_token: String,
    pub qovery_ssh_key: String,
    #[serde(default)]
    pub user_ssh_keys: Vec<String>,
    pub grafana_admin_user: String,
    pub grafana_admin_password: String,
    pub qovery_engine_location: EngineLocation,

    // Scaleway
    pub scaleway_project_id: String,
    pub scaleway_access_key: String,
    pub scaleway_secret_key: String,
    #[serde(default)]
    pub scaleway_kubernetes_type: KapsuleClusterType,

    // Other
    pub tls_email_report: String,
}

impl ProviderOptions for KapsuleOptions {}

impl KapsuleOptions {
    pub fn new(
        qovery_api_url: String,
        qovery_grpc_url: String,
        qovery_engine_url: String,
        qovery_cluster_jwt_token: String,
        qovery_ssh_key: String,
        grafana_admin_user: String,
        grafana_admin_password: String,
        qovery_engine_location: EngineLocation,
        scaleway_project_id: String,
        scaleway_access_key: String,
        scaleway_secret_key: String,
        tls_email_report: String,
        scaleway_kubernetes_type: KapsuleClusterType,
    ) -> KapsuleOptions {
        KapsuleOptions {
            qovery_api_url,
            qovery_grpc_url,
            qovery_engine_url,
            jwt_token: qovery_cluster_jwt_token,
            qovery_ssh_key,
            user_ssh_keys: vec![],
            grafana_admin_user,
            grafana_admin_password,
            qovery_engine_location,
            scaleway_project_id,
            scaleway_access_key,
            scaleway_secret_key,
            tls_email_report,
            scaleway_kubernetes_type,
        }
    }
}

pub struct Kapsule {
    context: Context,
    id: String,
    pub long_id: Uuid,
    name: String,
    pub version: KubernetesVersion,
    pub zone: ScwZone,
    pub object_storage: ScalewayOS,
    pub nodes_groups: Vec<NodeGroups>,
    pub template_directory: PathBuf,
    pub options: KapsuleOptions,
    logger: Box<dyn Logger>,
    advanced_settings: ClusterAdvancedSettings,
    pub customer_helm_charts_override: Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>>,
    kubeconfig: Option<String>,
    temp_dir: PathBuf,
}

impl Kapsule {
    pub fn new(
        context: Context,
        long_id: Uuid,
        name: String,
        version: KubernetesVersion,
        zone: ScwZone,
        cloud_provider: &dyn CloudProvider,
        nodes_groups: Vec<NodeGroups>,
        options: KapsuleOptions,
        logger: Box<dyn Logger>,
        advanced_settings: ClusterAdvancedSettings,
        customer_helm_charts_override: Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>>,
        kubeconfig: Option<String>,
        temp_dir: PathBuf,
    ) -> Result<Kapsule, Box<EngineError>> {
        let template_directory = PathBuf::from(context.lib_root_dir()).join("scaleway").join("bootstrap");
        let event_details = kubernetes::event_details(cloud_provider, long_id, name.to_string(), &context);

        for node_group in &nodes_groups {
            match ScwInstancesType::from_str(node_group.instance_type.as_str()) {
                Err(e) => {
                    let err = EngineError::new_unsupported_instance_type(
                        EventDetails::new(
                            Some(cloud_provider.kind()),
                            QoveryIdentifier::new(*context.organization_long_id()),
                            QoveryIdentifier::new(*context.cluster_long_id()),
                            context.execution_id().to_string(),
                            Infrastructure(InfrastructureStep::LoadConfiguration),
                            Transmitter::Kubernetes(long_id, name),
                        ),
                        node_group.instance_type.as_str(),
                        e,
                    );
                    logger.log(EngineEvent::Error(err.clone(), None));

                    return Err(Box::new(err));
                }
                Ok(instance_type) => {
                    if !instance_type.is_instance_cluster_allowed() {
                        let err = EngineError::new_unsupported_instance_type(
                            EventDetails::new(
                                Some(cloud_provider.kind()),
                                QoveryIdentifier::new(*context.organization_long_id()),
                                QoveryIdentifier::new(*context.cluster_long_id()),
                                context.execution_id().to_string(),
                                Infrastructure(InfrastructureStep::LoadConfiguration),
                                Transmitter::Kubernetes(long_id, name),
                            ),
                            node_group.instance_type.as_str(),
                            CommandError::new_from_safe_message(format!(
                                "`{instance_type}` instance type is not supported"
                            )),
                        );

                        return Err(Box::new(err));
                    }
                }
            }
        }

        advanced_settings.validate(event_details.clone())?;

        let object_storage = ScalewayOS::new(
            "s3-temp-id".to_string(),
            "default-s3".to_string(),
            cloud_provider.access_key_id(),
            cloud_provider.secret_access_key(),
            zone,
        );

        let cluster = Kapsule {
            context,
            id: to_short_id(&long_id),
            long_id,
            name,
            version,
            zone,
            object_storage,
            nodes_groups,
            template_directory,
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

    pub fn get_configuration(&self) -> scaleway_api_rs::apis::configuration::Configuration {
        scaleway_api_rs::apis::configuration::Configuration {
            api_key: Some(scaleway_api_rs::apis::configuration::ApiKey {
                key: self.options.scaleway_secret_key.clone(),
                prefix: None,
            }),
            ..scaleway_api_rs::apis::configuration::Configuration::default()
        }
    }

    pub fn get_scw_cluster_info(&self) -> Result<Option<ScalewayK8sV1Cluster>, Box<EngineError>> {
        let event_details = self.get_event_details(Infrastructure(InfrastructureStep::LoadConfiguration));

        // get cluster info
        let cluster_info = match block_on(scaleway_api_rs::apis::clusters_api::list_clusters(
            &self.get_configuration(),
            self.region(),
            None,
            Some(self.options.scaleway_project_id.as_str()),
            None,
            None,
            None,
            Some(self.cluster_name().as_str()),
            None,
            None,
        )) {
            Ok(x) => x,
            Err(e) => {
                return Err(Box::new(EngineError::new_cannot_get_cluster_error(
                    event_details,
                    CommandError::new(
                        "Error, wasn't able to retrieve SCW cluster information from the API.".to_string(),
                        Some(e.to_string()),
                        None,
                    ),
                )));
            }
        };

        // if no cluster exists
        let cluster_info_content = cluster_info.clusters.unwrap();
        if cluster_info_content.is_empty() {
            return Ok(None);
        } else if cluster_info_content.len() != 1_usize {
            return Err(Box::new(EngineError::new_multiple_cluster_found_expected_one_error(
                event_details,
                CommandError::new_from_safe_message(format!(
                    "Error, too many clusters found ({}) with this name, where 1 was expected.",
                    &cluster_info_content.len()
                )),
            )));
        }

        Ok(Some(cluster_info_content[0].clone()))
    }

    pub fn kubeconfig_bucket_name(&self) -> String {
        format!("qovery-kubeconfigs-{}", self.short_id())
    }

    pub fn logs_bucket_name(&self) -> String {
        format!("qovery-logs-{}", self.id)
    }
}

impl Kubernetes for Kapsule {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::ScwKapsule
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
        match self.zone {
            ScwZone::Paris1 => "fr-par",
            ScwZone::Paris2 => "fr-par",
            ScwZone::Paris3 => "fr-par",
            ScwZone::Amsterdam1 => "nl-ams",
            ScwZone::Amsterdam2 => "nl-ams",
            ScwZone::Amsterdam3 => "nl-ams",
            ScwZone::Warsaw1 => "pl-waw",
            ScwZone::Warsaw2 => "pl-waw",
            ScwZone::Warsaw3 => "pl-waw",
        }
    }

    fn zones(&self) -> Option<Vec<&str>> {
        Some(vec![self.zone.as_str()])
    }

    fn logger(&self) -> &dyn Logger {
        self.logger.borrow()
    }

    fn is_network_managed_by_user(&self) -> bool {
        false
    }

    fn cpu_architectures(&self) -> Vec<CpuArchitecture> {
        self.nodes_groups
            .iter()
            .map(|node| node.instance_architecture)
            .collect()
    }

    fn temp_dir(&self) -> &Path {
        &self.temp_dir
    }

    fn update_vault_config(
        &self,
        event_details: EventDetails,
        cluster_secrets: ClusterSecrets,
        kubeconfig_file_path: Option<&Path>,
    ) -> Result<(), Box<EngineError>> {
        let vault_conn = match QVaultClient::new(event_details.clone()) {
            Ok(x) => Some(x),
            Err(_) => None,
        };
        if let Some(vault) = vault_conn {
            // encode base64 kubeconfig
            let kubeconfig = match kubeconfig_file_path {
                Some(x) => fs::read_to_string(x)
                    .map_err(|e| {
                        EngineError::new_cannot_retrieve_cluster_config_file(
                            event_details.clone(),
                            CommandError::new_from_safe_message(format!(
                                "Cannot read kubeconfig file {}: {e}",
                                x.to_str().unwrap_or_default()
                            )),
                        )
                    })
                    .expect("kubeconfig was not found while it should be present"),
                None => "".to_string(),
            };
            let kubeconfig_b64 = general_purpose::STANDARD.encode(kubeconfig);
            let mut cluster_secrets_update = cluster_secrets;
            cluster_secrets_update.set_kubeconfig_b64(kubeconfig_b64);

            // update info without taking care of the kubeconfig because we don't have it yet
            let _ = cluster_secrets_update.create_or_update_secret(&vault, false, event_details);
        };
        Ok(())
    }

    fn advanced_settings(&self) -> &ClusterAdvancedSettings {
        &self.advanced_settings
    }

    fn loadbalancer_l4_annotations(&self, _cloud_provider_lb_name: Option<&str>) -> Vec<(String, String)> {
        // SCW doesn't support UDP loadbalancer
        // https://www.scaleway.com/en/docs/network/load-balancer/reference-content/configuring-backends/
        // https://www.scaleway.com/en/docs/containers/kubernetes/api-cli/using-load-balancer-annotations/
        vec![
            (
                "service.beta.kubernetes.io/scw-loadbalancer-forward-port-algorithm".to_string(),
                "leastconn".to_string(),
            ),
            (
                "service.beta.kubernetes.io/scw-loadbalancer-protocol-http".to_string(),
                "false".to_string(),
            ),
            (
                "service.beta.kubernetes.io/scw-loadbalancer-proxy-protocol-v1".to_string(),
                "false".to_string(),
            ),
            (
                "service.beta.kubernetes.io/scw-loadbalancer-proxy-protocol-v2".to_string(),
                "false".to_string(),
            ),
            (
                "service.beta.kubernetes.io/scw-loadbalancer-health-check-type".to_string(),
                "tcp".to_string(),
            ),
            (
                "service.beta.kubernetes.io/scw-loadbalancer-use-hostname".to_string(),
                "false".to_string(),
            ),
        ]
    }

    fn as_infra_actions(&self) -> &dyn InfrastructureAction {
        self
    }
}

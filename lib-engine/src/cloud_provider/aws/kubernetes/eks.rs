use crate::cloud_provider::aws::kubernetes;
use crate::cloud_provider::aws::kubernetes::ec2::mk_s3;
use crate::cloud_provider::aws::kubernetes::{KarpenterParameters, Options};
use crate::cloud_provider::aws::regions::{AwsRegion, AwsZone};
use crate::cloud_provider::io::ClusterAdvancedSettings;
use crate::cloud_provider::kubeconfig_helper::{fetch_kubeconfig, write_kubeconfig_on_disk};
use crate::cloud_provider::kubernetes::{event_details, Kind, Kubernetes, KubernetesVersion};
use crate::cloud_provider::models::CpuArchitecture;
use crate::cloud_provider::models::NodeGroups;
use crate::cloud_provider::CloudProvider;
use crate::errors::{CommandError, EngineError};
use crate::events::Stage::Infrastructure;
use crate::events::{EventDetails, InfrastructureStep};
use crate::io_models::context::Context;
use crate::io_models::engine_request::{ChartValuesOverrideName, ChartValuesOverrideValues};
use crate::logger::Logger;
use crate::models::ToCloudProviderFormat;
use crate::object_storage::s3::S3;
use crate::secret_manager::vault::QVaultClient;
use base64::engine::general_purpose;
use base64::Engine;
use std::borrow::Borrow;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::infrastructure_action::InfrastructureAction;
use crate::utilities::to_short_id;
use uuid::Uuid;

/// EKS kubernetes provider allowing to deploy an EKS cluster.
pub struct EKS {
    pub context: Context,
    pub id: String,
    pub long_id: Uuid,
    pub name: String,
    pub version: KubernetesVersion,
    pub region: AwsRegion,
    pub zones: Vec<AwsZone>,
    pub s3: S3,
    pub nodes_groups: Vec<NodeGroups>,
    pub template_directory: String,
    pub options: Options,
    pub logger: Box<dyn Logger>,
    pub advanced_settings: ClusterAdvancedSettings,
    pub customer_helm_charts_override: Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>>,
    pub kubeconfig: Option<String>,
    pub temp_dir: PathBuf,
    pub qovery_allowed_public_access_cidrs: Option<Vec<String>>,
}

impl EKS {
    pub fn new(
        context: Context,
        long_id: Uuid,
        name: &str,
        version: KubernetesVersion,
        region: AwsRegion,
        zones: Vec<String>,
        cloud_provider: &dyn CloudProvider,
        options: Options,
        nodes_groups: Vec<NodeGroups>,
        logger: Box<dyn Logger>,
        advanced_settings: ClusterAdvancedSettings,
        customer_helm_charts_override: Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>>,
        kubeconfig: Option<String>,
        temp_dir: PathBuf,
        qovery_allowed_public_access_cidrs: Option<Vec<String>>,
    ) -> Result<Self, Box<EngineError>> {
        let event_details = event_details(cloud_provider, long_id, name.to_string(), &context);
        let template_directory = format!("{}/aws/bootstrap", context.lib_root_dir());

        let aws_zones = kubernetes::aws_zones(zones, &region, &event_details)?;
        advanced_settings.validate(event_details.clone())?;

        let s3 = mk_s3(&region, cloud_provider);

        let cluster = EKS {
            context,
            id: to_short_id(&long_id),
            long_id,
            name: name.to_string(),
            version,
            region,
            zones: aws_zones,
            s3,
            options,
            nodes_groups,
            template_directory,
            logger,
            advanced_settings,
            customer_helm_charts_override,
            kubeconfig,
            temp_dir,
            qovery_allowed_public_access_cidrs,
        };

        if let Some(kubeconfig) = &cluster.kubeconfig {
            write_kubeconfig_on_disk(
                &cluster.kubeconfig_local_file_path(),
                kubeconfig,
                cluster.get_event_details(Infrastructure(InfrastructureStep::LoadConfiguration)),
            )?;
        } else {
            fetch_kubeconfig(&cluster, &cluster.s3)?;
        }

        Ok(cluster)
    }

    pub fn get_karpenter_parameters(&self) -> Option<KarpenterParameters> {
        if let Some(karpenter_parameters) = &self.options.karpenter_parameters {
            return Some(KarpenterParameters {
                spot_enabled: karpenter_parameters.spot_enabled,
                max_node_drain_time_in_secs: karpenter_parameters.max_node_drain_time_in_secs,
                disk_size_in_gib: karpenter_parameters.disk_size_in_gib,
                default_service_architecture: karpenter_parameters.default_service_architecture,
            });
        }

        if self.advanced_settings.aws_enable_karpenter {
            if let Some(node_group) = self.nodes_groups.first() {
                return Some(KarpenterParameters {
                    spot_enabled: self.advanced_settings.aws_karpenter_enable_spot,
                    max_node_drain_time_in_secs: self.advanced_settings.aws_karpenter_max_node_drain_in_sec,
                    disk_size_in_gib: node_group.disk_size_in_gib,
                    default_service_architecture: node_group.instance_architecture,
                });
            }
        }

        None
    }
}

impl Kubernetes for EKS {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::Eks
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

    fn is_valid(&self) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn is_network_managed_by_user(&self) -> bool {
        self.options.user_provided_network.is_some()
    }

    fn cpu_architectures(&self) -> Vec<CpuArchitecture> {
        if let Some(karpenter_parameters) = &self.options.karpenter_parameters {
            vec![karpenter_parameters.default_service_architecture]
        } else {
            self.nodes_groups.iter().map(|x| x.instance_architecture).collect()
        }
    }

    fn temp_dir(&self) -> &Path {
        &self.temp_dir
    }

    fn update_vault_config(
        &self,
        event_details: EventDetails,
        cluster_secrets: crate::cloud_provider::vault::ClusterSecrets,
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
            let _ = cluster_secrets_update.create_or_update_secret(&vault, false, event_details.clone());
        };

        Ok(())
    }

    fn advanced_settings(&self) -> &ClusterAdvancedSettings {
        &self.advanced_settings
    }

    fn customer_helm_charts_override(&self) -> Option<HashMap<ChartValuesOverrideName, ChartValuesOverrideValues>> {
        self.customer_helm_charts_override.clone()
    }

    fn is_karpenter_enabled(&self) -> bool {
        self.options.karpenter_parameters.is_some() || self.advanced_settings.aws_enable_karpenter
    }

    fn loadbalancer_l4_annotations(&self, cloud_provider_lb_name: Option<&str>) -> Vec<(String, String)> {
        let lb_name = match cloud_provider_lb_name {
            Some(x) => format!(",QoveryName={x}"),
            None => "".to_string(),
        };
        match self.advanced_settings().aws_eks_enable_alb_controller {
            // !!! IMPORTANT !!!
            // Changing this may require destroy/recreate a load balancer (and so downtime)
            true => {
                vec![
                    (
                        "service.beta.kubernetes.io/aws-load-balancer-type".to_string(),
                        "external".to_string(),
                    ),
                    (
                        "service.beta.kubernetes.io/aws-load-balancer-scheme".to_string(),
                        "internet-facing".to_string(),
                    ),
                    (
                        "service.beta.kubernetes.io/aws-load-balancer-nlb-target-type".to_string(),
                        "ip".to_string(),
                    ),
                    (
                        "service.beta.kubernetes.io/aws-load-balancer-additional-resource-tags".to_string(),
                        format!(
                            "OrganizationLongId={},OrganizationId={},ClusterLongId={},ClusterId={}{}",
                            self.context.organization_long_id(),
                            self.context.organization_short_id(),
                            self.long_id,
                            self.short_id(),
                            lb_name
                        ),
                    ),
                ]
            }
            false => vec![(
                "service.beta.kubernetes.io/aws-load-balancer-type".to_string(),
                "nlb".to_string(),
            )],
        }
    }

    fn as_infra_actions(&self) -> &dyn InfrastructureAction {
        self
    }

    fn as_eks(&self) -> Option<&EKS> {
        Some(self)
    }
}

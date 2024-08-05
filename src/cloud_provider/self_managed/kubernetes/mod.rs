use std::borrow::Borrow;
use std::path::{Path, PathBuf};
use std::{env, fs};

use base64::engine::general_purpose;
use base64::Engine;
use uuid::Uuid;

use crate::cloud_provider::aws::kubernetes::KarpenterParameters;
use crate::cloud_provider::io::ClusterAdvancedSettings;
use crate::cloud_provider::kubeconfig_helper::write_kubeconfig_on_disk;
use crate::cloud_provider::kubernetes::{self, Kind, Kubernetes, KubernetesVersion};
use crate::cloud_provider::models::CpuArchitecture;
use crate::cloud_provider::models::CpuArchitecture::AMD64;
use crate::cloud_provider::qovery::EngineLocation;
use crate::cloud_provider::CloudProvider;
use crate::cmd::docker;
use crate::engine::InfrastructureContext;
use crate::errors::{CommandError, EngineError};
use crate::events::InfrastructureStep;
use crate::events::Stage::Infrastructure;
use crate::io_models::context::Context;
use crate::logger::Logger;
use crate::secret_manager::vault::QVaultClient;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

pub struct SelfManaged {
    context: Context,
    id: String,
    kind: kubernetes::Kind,
    long_id: Uuid,
    name: String,
    version: KubernetesVersion,
    region: String,
    #[allow(dead_code)] //TODO(pmavro): not yet implemented
    options: SelfManagedOptions,
    logger: Box<dyn Logger>,
    advanced_settings: ClusterAdvancedSettings,
    kubeconfig: Option<String>,
    temp_dir: PathBuf,
}

impl SelfManaged {
    pub fn new(
        context: Context,
        id: String,
        long_id: Uuid,
        name: String,
        kind: Kind,
        version: KubernetesVersion,
        cloud_provider: &dyn CloudProvider,
        options: SelfManagedOptions,
        logger: Box<dyn Logger>,
        advanced_settings: ClusterAdvancedSettings,
        kubeconfig: Option<String>,
        temp_dir: PathBuf,
    ) -> Result<SelfManaged, Box<EngineError>> {
        let cluster = SelfManaged {
            context,
            id,
            kind,
            long_id,
            name,
            version,
            region: cloud_provider.region(),
            options,
            logger,
            advanced_settings,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelfManagedOptions {
    // Qovery
    pub qovery_grpc_url: String,
    #[serde(default)]
    pub qovery_engine_url: String,
    pub jwt_token: String,
    pub qovery_engine_location: EngineLocation,
}

impl Kubernetes for SelfManaged {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> kubernetes::Kind {
        self.kind
    }

    fn as_kubernetes(&self) -> &dyn Kubernetes {
        self
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
        self.region.as_str()
    }

    fn zones(&self) -> Option<Vec<&str>> {
        None
    }

    fn logger(&self) -> &dyn Logger {
        self.logger.borrow()
    }

    fn is_valid(&self) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn is_network_managed_by_user(&self) -> bool {
        true
    }

    fn is_self_managed(&self) -> bool {
        true
    }

    fn cpu_architectures(&self) -> Vec<CpuArchitecture> {
        match self.kind {
            Kind::Eks
            | Kind::Ec2
            | Kind::ScwKapsule
            | Kind::Gke
            | Kind::EksSelfManaged
            | Kind::GkeSelfManaged
            | Kind::ScwSelfManaged => vec![AMD64], // we cant know for now so we fall back to amd64
            Kind::OnPremiseSelfManaged => {
                // We take what is configured by the engine, if nothing is configured we default to amd64
                info!("BUILDER_CPU_ARCHITECTURES: {:?}", env::var("BUILDER_CPU_ARCHITECTURES"));
                let archs: Vec<CpuArchitecture> = env::var("BUILDER_CPU_ARCHITECTURES")
                    .unwrap_or_default()
                    .split(',')
                    .filter_map(|x| docker::Architecture::from_str(x).ok())
                    .map(|x| match x {
                        docker::Architecture::AMD64 => AMD64,
                        docker::Architecture::ARM64 => CpuArchitecture::ARM64,
                    })
                    .collect();
                info!("BUILDER_CPU_ARCHITECTURES: {:?}", archs);

                if archs.is_empty() {
                    vec![AMD64]
                } else {
                    archs
                }
            }
        }
    }

    fn on_create(&self, _infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn upgrade_with_status(
        &self,
        _infra_ctx: &InfrastructureContext,
        _kubernetes_upgrade_status: kubernetes::KubernetesUpgradeStatus,
    ) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn on_pause(&self, _infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        Ok(())
    }

    fn on_delete(&self, _infra_ctx: &InfrastructureContext) -> Result<(), Box<EngineError>> {
        Ok(())
    }
    fn temp_dir(&self) -> &Path {
        &self.temp_dir
    }

    fn update_vault_config(
        &self,
        event_details: crate::events::EventDetails,
        _qovery_terraform_config_file: String,
        cluster_secrets: crate::cloud_provider::vault::ClusterSecrets,
        kubeconfig_file_path: Option<&std::path::Path>,
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

    fn customer_helm_charts_override(
        &self,
    ) -> Option<
        std::collections::HashMap<
            crate::io_models::engine_request::ChartValuesOverrideName,
            crate::io_models::engine_request::ChartValuesOverrideValues,
        >,
    > {
        None
    }

    fn is_karpenter_enabled(&self) -> bool {
        false
    }

    fn get_karpenter_parameters(&self) -> Option<KarpenterParameters> {
        None
    }

    fn loadbalancer_l4_annotations(&self) -> &'static [(&'static str, &'static str)] {
        &[]
    }
}

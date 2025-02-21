use std::borrow::Borrow;
use std::env;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::cmd::docker;
use crate::errors::EngineError;
use crate::infrastructure::action::InfrastructureAction;
use crate::infrastructure::models::cloud_provider::io::ClusterAdvancedSettings;
use crate::infrastructure::models::kubernetes::{self, Kind, Kubernetes, KubernetesVersion};
use crate::io_models::context::Context;
use crate::io_models::engine_location::EngineLocation;
use crate::io_models::models::CpuArchitecture;
use crate::io_models::models::CpuArchitecture::{AMD64, ARM64};
use crate::logger::Logger;
use crate::utilities::to_short_id;
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
    _kubeconfig: Option<String>,
    temp_dir: PathBuf,
}

impl SelfManaged {
    pub fn new(
        context: Context,
        long_id: Uuid,
        name: String,
        kind: Kind,
        region: String,
        version: KubernetesVersion,
        options: SelfManagedOptions,
        logger: Box<dyn Logger>,
        advanced_settings: ClusterAdvancedSettings,
        kubeconfig: Option<String>,
        temp_dir: PathBuf,
    ) -> Result<SelfManaged, Box<EngineError>> {
        let cluster = SelfManaged {
            context,
            id: to_short_id(&long_id),
            kind,
            long_id,
            name,
            version,
            region,
            options,
            logger,
            advanced_settings,
            _kubeconfig: kubeconfig,
            temp_dir,
        };

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
        self.region.as_str()
    }

    fn zones(&self) -> Option<Vec<&str>> {
        None
    }

    fn logger(&self) -> &dyn Logger {
        self.logger.borrow()
    }

    fn is_network_managed_by_user(&self) -> bool {
        true
    }

    fn cpu_architectures(&self) -> Vec<CpuArchitecture> {
        // We take what is configured by the engine, if nothing is configured we default to amd64
        info!("BUILDER_CPU_ARCHITECTURES: {:?}", env::var("BUILDER_CPU_ARCHITECTURES"));
        let archs: Vec<CpuArchitecture> = env::var("BUILDER_CPU_ARCHITECTURES")
            .unwrap_or_default()
            .split(',')
            .filter_map(|x| docker::Architecture::from_str(x).ok())
            .map(|x| match x {
                docker::Architecture::AMD64 => AMD64,
                docker::Architecture::ARM64 => ARM64,
            })
            .collect();
        info!("BUILDER_CPU_ARCHITECTURES: {:?}", archs);

        if archs.is_empty() { vec![AMD64] } else { archs }
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

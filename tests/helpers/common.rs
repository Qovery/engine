extern crate serde;
extern crate serde_derive;

use crate::helpers::utilities::FuncTestsSecrets;

use qovery_engine::environment::models::environment::Environment;
use qovery_engine::errors::EngineError;
use qovery_engine::infrastructure::infrastructure_context::InfrastructureContext;
use qovery_engine::infrastructure::models::kubernetes::karpenter::KarpenterParameters;
use qovery_engine::infrastructure::models::kubernetes::{Kind as KubernetesKind, KubernetesVersion};
use qovery_engine::io_models::context::Context;
use qovery_engine::io_models::engine_location::EngineLocation;
use qovery_engine::io_models::models::{CpuArchitecture, NodeGroups, VpcQoveryNetworkMode};

use qovery_engine::io_models::QoveryIdentifier;
use qovery_engine::io_models::environment::EnvironmentRequest;
use qovery_engine::logger::Logger;
use qovery_engine::metrics_registry::MetricsRegistry;

pub const DEFAULT_RESOURCE_TTL_IN_SECONDS: u32 = 7200;
pub const DEFAULT_QUICK_RESOURCE_TTL_IN_SECONDS: u32 = 3600;

pub enum RegionActivationStatus {
    Deactivated,
    Activated,
}

#[derive(Clone)]
pub enum ClusterDomain {
    Default { cluster_id: String },
    QoveryOwnedDomain { cluster_id: String, domain: String },
    Custom { domain: String },
}

#[derive(Clone)]
pub enum NodeManager {
    Default,
    Karpenter { config: KarpenterParameters },
    AutoPilot,
}

/// Represents a feature that can be enabled at demand
/// When specified, the given `ActionableFeature`(s) will be:
/// - enabled at cluster update (after cluster creation)
/// - disabled after cluster update (before cluster deletion)
pub enum ActionableFeature {
    Metrics,
}

pub trait Cluster<T, U> {
    fn docker_cr_engine(
        context: &Context,
        logger: Box<dyn Logger>,
        metrics_registry: Box<dyn MetricsRegistry>,
        localisation: &str,
        kubernetes_kind: KubernetesKind,
        kubernetes_version: KubernetesVersion,
        cluster_domain: &ClusterDomain,
        vpc_network_mode: Option<VpcQoveryNetworkMode>,
        min_nodes: i32,
        max_nodes: i32,
        cpu_archi: CpuArchitecture,
        engine_location: EngineLocation,
        kubeconfig: Option<String>,
        node_manager: NodeManager,
        actionable_features: Vec<ActionableFeature>,
    ) -> InfrastructureContext;
    fn cloud_provider(context: &Context, kubernetes_kind: KubernetesKind, localisation: &str) -> Box<T>;
    fn kubernetes_nodes(min_nodes: i32, max_nodes: i32, cpu_archi: CpuArchitecture) -> Vec<NodeGroups>;
    fn kubernetes_cluster_options(
        secrets: FuncTestsSecrets,
        cluster_id: QoveryIdentifier,
        engine_location: EngineLocation,
        vpc_network_mode: Option<VpcQoveryNetworkMode>,
    ) -> U;
}

pub trait Infrastructure {
    fn build_environment(
        &self,
        environment: &EnvironmentRequest,
        infra_ctx: &InfrastructureContext,
    ) -> (Environment, Result<(), Box<EngineError>>);

    fn deploy_environment(
        &self,
        environment: &EnvironmentRequest,
        infra_ctx: &InfrastructureContext,
    ) -> Result<(), Box<EngineError>>;

    fn pause_environment(
        &self,
        environment: &EnvironmentRequest,
        infra_ctx: &InfrastructureContext,
    ) -> Result<(), Box<EngineError>>;

    fn delete_environment(
        &self,
        environment: &EnvironmentRequest,
        infra_ctx: &InfrastructureContext,
    ) -> Result<(), Box<EngineError>>;

    fn restart_environment(
        &self,
        environment: &EnvironmentRequest,
        infra_ctx: &InfrastructureContext,
    ) -> Result<(), Box<EngineError>>;
}

pub(crate) fn compute_test_cluster_endpoint(cluster_domain: &ClusterDomain, default_domain: String) -> String {
    match cluster_domain {
        ClusterDomain::Default { cluster_id } => format!("{cluster_id}.{default_domain}"),
        ClusterDomain::QoveryOwnedDomain { cluster_id, domain } => format!("{cluster_id}.{domain}"),
        ClusterDomain::Custom { domain } => domain.to_string(),
    }
}

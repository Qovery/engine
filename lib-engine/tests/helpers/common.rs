extern crate serde;
extern crate serde_derive;

use crate::helpers::utilities::FuncTestsSecrets;

use qovery_engine::cloud_provider::aws::kubernetes::VpcQoveryNetworkMode;
use qovery_engine::cloud_provider::environment::Environment;
use qovery_engine::cloud_provider::kubernetes::Kind as KubernetesKind;
use qovery_engine::cloud_provider::models::NodeGroups;
use qovery_engine::cloud_provider::qovery::EngineLocation;
use qovery_engine::engine::EngineConfig;

use qovery_engine::io_models::context::Context;

use qovery_engine::io_models::environment::EnvironmentRequest;
use qovery_engine::logger::Logger;
use qovery_engine::transaction::TransactionResult;

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

pub trait Cluster<T, U> {
    fn docker_cr_engine(
        context: &Context,
        logger: Box<dyn Logger>,
        localisation: &str,
        kubernetes_kind: KubernetesKind,
        kubernetes_version: String,
        cluster_domain: &ClusterDomain,
        vpc_network_mode: Option<VpcQoveryNetworkMode>,
        min_nodes: i32,
        max_nodes: i32,
        engine_location: EngineLocation,
    ) -> EngineConfig;
    fn cloud_provider(context: &Context, kubernetes_kind: KubernetesKind) -> Box<T>;
    fn kubernetes_nodes(min_nodes: i32, max_nodes: i32) -> Vec<NodeGroups>;
    fn kubernetes_cluster_options(
        secrets: FuncTestsSecrets,
        cluster_id: Option<String>,
        engine_location: EngineLocation,
    ) -> U;
}

pub trait Infrastructure {
    fn build_environment(
        &self,
        environment: &EnvironmentRequest,
        logger: Box<dyn Logger>,
        engine_config: &EngineConfig,
    ) -> (Environment, TransactionResult);

    fn deploy_environment(
        &self,
        environment: &EnvironmentRequest,
        logger: Box<dyn Logger>,
        engine_config: &EngineConfig,
    ) -> TransactionResult;

    fn pause_environment(
        &self,
        environment: &EnvironmentRequest,
        logger: Box<dyn Logger>,
        engine_config: &EngineConfig,
    ) -> TransactionResult;

    fn delete_environment(
        &self,
        environment: &EnvironmentRequest,
        logger: Box<dyn Logger>,
        engine_config: &EngineConfig,
    ) -> TransactionResult;
}

pub(crate) fn compute_test_cluster_endpoint(cluster_domain: &ClusterDomain, default_domain: String) -> String {
    match cluster_domain {
        ClusterDomain::Default { cluster_id } => format!("{}.{}", cluster_id, default_domain),
        ClusterDomain::QoveryOwnedDomain { cluster_id, domain } => format!("{}.{}", cluster_id, domain),
        ClusterDomain::Custom { domain } => domain.to_string(),
    }
}

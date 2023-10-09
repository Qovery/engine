use crate::helpers::common::{Cluster, ClusterDomain};
use crate::helpers::utilities::FuncTestsSecrets;
use lazy_static::lazy_static;
use qovery_engine::cloud_provider::aws::AWS;
use qovery_engine::cloud_provider::kubernetes::{Kind as KubernetesKind, KubernetesVersion};
use qovery_engine::cloud_provider::models::{CpuArchitecture, InstanceEc2};
use qovery_engine::cloud_provider::qovery::EngineLocation;
use qovery_engine::container_registry::ecr::ECR;
use qovery_engine::engine::InfrastructureContext;
use qovery_engine::io_models::context::Context;
use qovery_engine::logger::Logger;
use qovery_engine::metrics_registry::MetricsRegistry;
use std::string::ToString;
use std::sync::Arc;
use tracing::error;
use uuid::Uuid;

pub const AWS_EC2_KUBERNETES_MIN_NODES: i32 = 1;
pub const AWS_EC2_KUBERNETES_MAX_NODES: i32 = 1;

lazy_static! {
    pub static ref AWS_EC2_KUBERNETES_VERSION: KubernetesVersion = KubernetesVersion::V1_26 {
        prefix: Some(Arc::from("v")),
        patch: Some(6),
        suffix: Some(Arc::from("+k3s1")),
    };
}

pub fn ec2_kubernetes_instance() -> InstanceEc2 {
    InstanceEc2::new("t3.large".to_string(), 20, CpuArchitecture::AMD64)
}

pub fn container_registry_ecr_ec2(context: &Context, logger: Box<dyn Logger>, region: &str) -> ECR {
    let secrets = FuncTestsSecrets::new();
    if secrets.AWS_ACCESS_KEY_ID.is_none() || secrets.AWS_SECRET_ACCESS_KEY.is_none() {
        error!("Please check your Vault connectivity (token/address) or AWS_ACCESS_KEY_ID/AWS_SECRET_ACCESS_KEY environment variables are set");
        std::process::exit(1)
    }

    ECR::new(
        context.clone(),
        format!("default-ecr-ec2-registry-{region}-Qovery Test").as_str(),
        Uuid::new_v4(),
        "ea69qe62xaw3wjai",
        secrets.AWS_ACCESS_KEY_ID.unwrap().as_str(),
        secrets.AWS_SECRET_ACCESS_KEY.unwrap().as_str(),
        region,
        logger,
        hashmap! {},
    )
    .unwrap()
}

pub fn aws_ec2_default_infra_config(
    context: &Context,
    logger: Box<dyn Logger>,
    metrics_registry: Box<dyn MetricsRegistry>,
) -> InfrastructureContext {
    let secrets = FuncTestsSecrets::new();

    AWS::docker_cr_engine(
        context,
        logger,
        metrics_registry,
        secrets
            .AWS_EC2_TEST_CLUSTER_REGION
            .expect("AWS_EC2_TEST_CLUSTER_REGION is not set")
            .as_str(),
        KubernetesKind::Ec2,
        AWS_EC2_KUBERNETES_VERSION.clone(),
        &ClusterDomain::Default {
            cluster_id: context.cluster_short_id().to_string(),
        },
        None,
        AWS_EC2_KUBERNETES_MIN_NODES,
        AWS_EC2_KUBERNETES_MAX_NODES,
        CpuArchitecture::AMD64,
        EngineLocation::QoverySide,
    )
}

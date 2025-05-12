use crate::helpers::common::{ActionableFeature, Cluster, ClusterDomain, NodeManager};
use crate::helpers::dns::dns_provider_qoverydns;
use crate::helpers::kubernetes::{
    KUBERNETES_MAX_NODES, KUBERNETES_MIN_NODES, TargetCluster, get_environment_test_kubernetes,
};
use crate::helpers::utilities::{FuncTestsSecrets, build_platform_local_docker};
use azure_mgmt_containerregistry::models::{Sku, sku};
use governor::middleware::NoOpMiddleware;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Quota, RateLimiter, clock};
use nonzero_ext::nonzero;
use once_cell::sync::Lazy;
use qovery_engine::environment::models::azure::{AzureStorageType, Credentials};
use qovery_engine::infrastructure::infrastructure_context::InfrastructureContext;
use qovery_engine::infrastructure::models::cloud_provider::TerraformStateCredentials;
use qovery_engine::infrastructure::models::cloud_provider::azure::Azure;
use qovery_engine::infrastructure::models::cloud_provider::azure::locations::AzureLocation;
use qovery_engine::infrastructure::models::container_registry::ContainerRegistry;
use qovery_engine::infrastructure::models::container_registry::azure_container_registry::AzureContainerRegistry;
use qovery_engine::infrastructure::models::container_registry::errors::ContainerRegistryError;
use qovery_engine::infrastructure::models::kubernetes::azure::AksOptions;
use qovery_engine::infrastructure::models::kubernetes::{Kind as KubernetesKind, KubernetesVersion};
use qovery_engine::io_models::context::Context;
use qovery_engine::io_models::engine_location::EngineLocation;
use qovery_engine::io_models::environment::EnvironmentRequest;
use qovery_engine::io_models::models::{CpuArchitecture, NodeGroups, StorageClass, VpcQoveryNetworkMode};
use qovery_engine::logger::Logger;
use qovery_engine::metrics_registry::MetricsRegistry;
use qovery_engine::services::azure::container_registry_service::AzureContainerRegistryService;
use std::str::FromStr;
use std::sync::Arc;

pub const AZURE_KUBERNETES_VERSION: KubernetesVersion = KubernetesVersion::V1_31 {
    prefix: None,
    patch: None,
    suffix: None,
};
pub const AZURE_RESOURCE_GROUP_NAME: &str = "qovery_test";
pub const AZURE_LOCATION: AzureLocation = AzureLocation::FranceCentral;
pub static AZURE_CONTAINER_REGISTRY_SKU: Lazy<Sku> = Lazy::new(|| Sku::new(sku::Name::Basic));
pub const AZURE_RESOURCE_TTL_IN_SECONDS: u32 = 9000;
pub const AZURE_SELF_HOSTED_DATABASE_DISK_TYPE: AzureStorageType = AzureStorageType::StandardSSDZRS;

/// A rate limiter making sure we do not send too many repository writes requests while testing
/// Max default quotas are 100 RPM on Basic SKU, let's take some room and use 10x less (1 per 10 seconds)
/// more info here hhttps://learn.microsoft.com/en-us/azure/container-registry/container-registry-skus
pub static AZURE_ARTIFACT_REGISTRY_REPOSITORY_WRITE_RATE_LIMITER: Lazy<
    Arc<RateLimiter<NotKeyed, InMemoryState, clock::DefaultClock, NoOpMiddleware>>,
> = Lazy::new(|| Arc::from(RateLimiter::direct(Quota::per_minute(nonzero!(10_u32)))));

/// A rate limiter making sure we do not send too many repository writes requests while testing
/// Max default quotas are 1000 RPM on Basic SKU, let's take some room and use 20x less (1 per 2 seconds)
/// more info here hhttps://learn.microsoft.com/en-us/azure/container-registry/container-registry-skus
pub static AZURE_ARTIFACT_REGISTRY_REPOSITORY_READ_RATE_LIMITER: Lazy<
    Arc<RateLimiter<NotKeyed, InMemoryState, clock::DefaultClock, NoOpMiddleware>>,
> = Lazy::new(|| Arc::from(RateLimiter::direct(Quota::per_minute(nonzero!(30_u32)))));

pub fn azure_container_registry(context: &Context) -> AzureContainerRegistry {
    let secrets = FuncTestsSecrets::new();

    // For Azure, there will be only one container registry per cluster
    // it will be named after the cluster id
    let id = context.cluster_long_id();
    let name = format!("qovery-{}", context.cluster_short_id());

    AzureContainerRegistry::new(
        context.clone(),
        *id,
        name.as_str(),
        secrets
            .AZURE_SUBSCRIPTION_ID
            .as_ref()
            .expect("AZURE_SUBSCRIPTION_ID is not set in secrets"),
        AZURE_RESOURCE_GROUP_NAME,
        secrets
            .AZURE_CLIENT_ID
            .as_ref()
            .expect("AZURE_CLIENT_ID is not set in secrets"),
        secrets
            .AZURE_CLIENT_SECRET
            .as_ref()
            .expect("AZURE_CLIENT_SECRET is not set in secrets"),
        AZURE_LOCATION,
        Arc::new(
            AzureContainerRegistryService::new(
                secrets
                    .AZURE_TENANT_ID
                    .as_ref()
                    .expect("AZURE_TENANT_ID is not set in secrets"),
                secrets
                    .AZURE_CLIENT_ID
                    .as_ref()
                    .expect("AZURE_CLIENT_ID is not set in secrets"),
                secrets
                    .AZURE_CLIENT_SECRET
                    .as_ref()
                    .expect("AZURE_CLIENT_SECRET is not set in secrets"),
                Some(AZURE_ARTIFACT_REGISTRY_REPOSITORY_WRITE_RATE_LIMITER.clone()),
                Some(AZURE_ARTIFACT_REGISTRY_REPOSITORY_READ_RATE_LIMITER.clone()),
            )
            .expect("Cannot create Azure Artifact registry service"),
        ),
    )
    .expect("Cannot create Azure Artifact Registry")
}

pub fn azure_infra_config(
    targeted_cluster: &TargetCluster,
    context: &Context,
    logger: Box<dyn Logger>,
    metrics_registry: Box<dyn MetricsRegistry>,
) -> InfrastructureContext {
    let secrets = FuncTestsSecrets::new();

    Azure::docker_cr_engine(
        context,
        logger,
        metrics_registry,
        secrets
            .AZURE_DEFAULT_REGION
            .expect("AZURE_DEFAULT_REGION is not set in secrets")
            .as_str(),
        KubernetesKind::Gke,
        AZURE_KUBERNETES_VERSION,
        &ClusterDomain::Default {
            cluster_id: context.cluster_short_id().to_string(),
        },
        None,
        KUBERNETES_MIN_NODES,
        KUBERNETES_MAX_NODES,
        CpuArchitecture::AMD64,
        EngineLocation::ClientSide,
        match targeted_cluster {
            TargetCluster::MutualizedTestCluster { kubeconfig } => Some(kubeconfig.to_string()), // <- using test cluster, not creating a new one
            TargetCluster::New => None, // <- creating a new cluster
        },
        NodeManager::Default,
        vec![],
    )
}

impl Cluster<Azure, AksOptions> for Azure {
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
    ) -> InfrastructureContext {
        // use Azure container registry
        let container_registry = ContainerRegistry::AzureContainerRegistry(azure_container_registry(context));

        // use LocalDocker
        let build_platform = Box::new(build_platform_local_docker(context));

        // Use Azure
        let cloud_provider = Azure::cloud_provider(context, kubernetes_kind, localisation);
        let dns_provider = dns_provider_qoverydns(context, cluster_domain);

        let kubernetes = get_environment_test_kubernetes(
            context,
            cloud_provider.as_ref(),
            kubernetes_version,
            logger.clone(),
            localisation,
            vpc_network_mode,
            min_nodes,
            max_nodes,
            cpu_archi,
            engine_location,
            StorageClass(AzureStorageType::StandardSSDZRS.to_k8s_storage_class()),
            kubeconfig,
            node_manager,
            actionable_features,
        );

        InfrastructureContext::new(
            context.clone(),
            build_platform,
            container_registry,
            cloud_provider,
            dns_provider,
            kubernetes,
            metrics_registry,
            true,
        )
    }

    fn cloud_provider(context: &Context, _kubernetes_kind: KubernetesKind, localisation: &str) -> Box<Azure> {
        let secrets = FuncTestsSecrets::new();

        Box::new(Azure::new(
            *context.cluster_long_id(),
            AzureLocation::from_str(localisation).expect("Unknown Azure location"),
            Credentials {
                client_id: secrets.AZURE_CLIENT_ID.expect("AZURE_CLIENT_ID is not set"),
                client_secret: secrets.AZURE_CLIENT_SECRET.expect("AZURE_CLIENT_SECRET is not set"),
                tenant_id: secrets.AZURE_TENANT_ID.expect("AZURE_TENANT_ID is not set"),
                subscription_id: secrets.AZURE_SUBSCRIPTION_ID.expect("AZURE_SUBSCRIPTION_ID is not set"),
                resource_group_name: format!("qovery-{}", context.cluster_short_id()), // one per cluster
            },
            TerraformStateCredentials {
                access_key_id: secrets
                    .TERRAFORM_AWS_ACCESS_KEY_ID
                    .expect("TERRAFORM_AWS_ACCESS_KEY_ID is n ot set"),
                secret_access_key: secrets
                    .TERRAFORM_AWS_SECRET_ACCESS_KEY
                    .expect("TERRAFORM_AWS_SECRET_ACCESS_KEY is not set"),
                region: secrets.TERRAFORM_AWS_REGION.expect("TERRAFORM_AWS_REGION is not set"),
                s3_bucket: secrets.TERRAFORM_AWS_BUCKET.expect("TERRAFORM_AWS_BUCKET is not set"),
                dynamodb_table: secrets
                    .TERRAFORM_AWS_DYNAMODB_TABLE
                    .expect("TERRAFORM_AWS_DYNAMODB_TABLE is not set"),
            },
        ))
    }

    fn kubernetes_nodes(min_nodes: i32, max_nodes: i32, cpu_archi: CpuArchitecture) -> Vec<NodeGroups> {
        // TODO(benjaminch): add support for more instance type ARM64

        vec![
            NodeGroups::new(
                "default1".to_string(),
                min_nodes,
                max_nodes,
                "Standard_D2s_v3".to_string(),
                100,
                cpu_archi,
            )
            .expect("Problem while setup AKS nodes"),
            NodeGroups::new(
                "default2".to_string(),
                min_nodes,
                max_nodes,
                "Standard_D2s_v3".to_string(),
                100,
                cpu_archi,
            )
            .expect("Problem while setup AKS nodes"),
            NodeGroups::new(
                "default3".to_string(),
                min_nodes,
                max_nodes,
                "Standard_D2s_v3".to_string(),
                100,
                cpu_archi,
            )
            .expect("Problem while setup AKS nodes"),
        ]
    }

    fn kubernetes_cluster_options(
        secrets: FuncTestsSecrets,
        _cluster_id: Option<String>,
        engine_location: EngineLocation,
        _vpc_network_mode: Option<VpcQoveryNetworkMode>,
    ) -> AksOptions {
        AksOptions::new(
            secrets.QOVERY_API_URL.expect("QOVERY_API_URL is not set in secrets"),
            secrets
                .QOVERY_GRPC_URL
                .clone()
                .expect("QOVERY_GRPC_URL is not set in secrets"),
            secrets
                .ENGINE_SERVER_URL
                .expect("ENGINE_SERVER_URL is not set in secrets"),
            secrets
                .QOVERY_CLUSTER_JWT_TOKEN
                .expect("QOVERY_CLUSTER_JWT_TOKEN is not set in secrets"),
            secrets.QOVERY_SSH_USER.expect("QOVERY_SSH_USER is not set in secrets"),
            Vec::with_capacity(0),
            "admin".to_string(),
            "qovery".to_string(),
            engine_location,
            secrets
                .LETS_ENCRYPT_EMAIL_REPORT
                .expect("LETS_ENCRYPT_EMAIL_REPORT is not set in secrets"),
            None,
        )
    }
}

pub fn clean_environments(
    _context: &Context,
    _environments: Vec<EnvironmentRequest>,
    _region: AzureLocation,
) -> Result<(), ContainerRegistryError> {
    let _secrets = FuncTestsSecrets::new();

    // delete repository created in registry
    // TODO(benjaminch): delete repository

    Ok(())
}

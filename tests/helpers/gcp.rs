use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use governor::middleware::NoOpMiddleware;
use governor::state::{InMemoryState, NotKeyed};
use governor::{clock, Quota, RateLimiter};
use nonzero_ext::nonzero;
use once_cell::sync::Lazy;
use time::Time;
use uuid::Uuid;

use qovery_engine::cloud_provider::gcp::kubernetes::{Gke, GkeOptions, VpcMode};
use qovery_engine::cloud_provider::gcp::locations::GcpRegion;
use qovery_engine::cloud_provider::gcp::Google;
use qovery_engine::cloud_provider::kubernetes::{Kind as KubernetesKind, KubernetesVersion};
use qovery_engine::cloud_provider::models::{CpuArchitecture, NodeGroups, VpcQoveryNetworkMode};
use qovery_engine::cloud_provider::qovery::EngineLocation;
use qovery_engine::cloud_provider::{CloudProvider, TerraformStateCredentials};
use qovery_engine::container_registry::errors::ContainerRegistryError;
use qovery_engine::container_registry::google_artifact_registry::GoogleArtifactRegistry;
use qovery_engine::dns_provider::DnsProvider;
use qovery_engine::engine::InfrastructureContext;
use qovery_engine::io_models::context::Context;
use qovery_engine::io_models::environment::EnvironmentRequest;
use qovery_engine::io_models::QoveryIdentifier;
use qovery_engine::logger::Logger;
use qovery_engine::metrics_registry::MetricsRegistry;
use qovery_engine::models::gcp::io::JsonCredentials as JsonCredentialsIo;
use qovery_engine::models::gcp::{GcpStorageType, JsonCredentials};
use qovery_engine::services::gcp::artifact_registry_service::ArtifactRegistryService;

use crate::helpers::common::{Cluster, ClusterDomain};
use crate::helpers::dns::dns_provider_qoverydns;
use crate::helpers::kubernetes::get_environment_test_kubernetes;
use crate::helpers::utilities::{build_platform_local_docker, FuncTestsSecrets};

pub const GCP_REGION: GcpRegion = GcpRegion::EuropeWest9;

pub const GCP_SELF_HOSTED_DATABASE_DISK_TYPE: GcpStorageType = GcpStorageType::Balanced;
pub const GCP_MANAGED_DATABASE_DISK_TYPE: &str = "";
// TODO: once managed DB is implemented
pub const GCP_MANAGED_DATABASE_INSTANCE_TYPE: &str = ""; // TODO: once managed DB is implemented

pub static GCP_RESOURCE_TTL: Lazy<Duration> = Lazy::new(|| Duration::from_secs(4 * 60 * 60)); // 4 hours

pub const GCP_KUBERNETES_VERSION: KubernetesVersion = KubernetesVersion::V1_29 {
    prefix: None,
    patch: None,
    suffix: None,
};

/// A rate limiter making sure we do not send too many repository writes requests while testing
/// Max default quotas are 0.5 RPS, let's take some room and use 10x less (1 per 10 seconds)
/// more info here https://cloud.google.com/artifact-registry/quotas
pub static GCP_ARTIFACT_REGISTRY_REPOSITORY_API_OBJECT_WRITE_RATE_LIMITER: Lazy<
    Arc<RateLimiter<NotKeyed, InMemoryState, clock::DefaultClock, NoOpMiddleware>>,
> = Lazy::new(|| Arc::from(RateLimiter::direct(Quota::per_minute(nonzero!(10_u32)))));

/// A rate limiter making sure we do not send too many repository images writes requests while testing
/// Max default quotas are 0.5 RPS, let's take some room and use 10x less (1 per 10 seconds)
/// more info here https://cloud.google.com/artifact-registry/quotas
pub static GCP_ARTIFACT_REGISTRY_IMAGE_API_OBJECT_WRITE_RATE_LIMITER: Lazy<
    Arc<RateLimiter<NotKeyed, InMemoryState, clock::DefaultClock, NoOpMiddleware>>,
> = Lazy::new(|| Arc::from(RateLimiter::direct(Quota::per_minute(nonzero!(10_u32)))));

/// A rate limiter making sure we do not send too many bucket write requests while testing
/// Max default quotas are 0.5 RPS, let's take some room and use 10x less (1 per 12 seconds)
/// more info here https://cloud.google.com/storage/quotas?hl=fr
pub static GCP_STORAGE_API_BUCKET_WRITE_RATE_LIMITER: Lazy<
    Arc<RateLimiter<NotKeyed, InMemoryState, clock::DefaultClock, NoOpMiddleware>>,
> = Lazy::new(|| Arc::from(RateLimiter::direct(Quota::per_minute(nonzero!(5_u32)))));

/// A rate limiter making sure we do not send too many object write requests while testing
/// Max default quotas are 1 RPS, let's take some room and use 10x less (1 per 6 seconds)
/// more info here https://cloud.google.com/storage/quotas?hl=fr
pub static GCP_STORAGE_API_OBJECT_WRITE_RATE_LIMITER: Lazy<
    Arc<RateLimiter<NotKeyed, InMemoryState, clock::DefaultClock, NoOpMiddleware>>,
> = Lazy::new(|| Arc::from(RateLimiter::direct(Quota::per_minute(nonzero!(10_u32)))));

pub fn gcp_container_registry(context: &Context) -> GoogleArtifactRegistry {
    let secrets = FuncTestsSecrets::new();

    let credentials = try_parse_json_credentials_from_str(
        secrets
            .GCP_CREDENTIALS
            .as_ref()
            .expect("GCP_CREDENTIALS is not set in secrets"),
    )
    .expect("Cannot parse GCP_CREDENTIALS");

    let id = QoveryIdentifier::new_random();
    let name = format!("test-artifact-registry-{id}");
    let region = GcpRegion::from_str(
        secrets
            .GCP_DEFAULT_REGION
            .as_ref()
            .expect("GCP_DEFAULT_REGION is not set in secrets"),
    )
    .expect("Unknown GCP region");

    GoogleArtifactRegistry::new(
        context.clone(),
        id.to_uuid(),
        &name,
        secrets
            .GCP_PROJECT_NAME
            .as_ref()
            .expect("GCP_PROJECT_NAME is not set in secrets"),
        region,
        credentials.clone(),
        Arc::new(
            ArtifactRegistryService::new(
                credentials,
                Some(GCP_ARTIFACT_REGISTRY_REPOSITORY_API_OBJECT_WRITE_RATE_LIMITER.clone()),
                Some(GCP_ARTIFACT_REGISTRY_IMAGE_API_OBJECT_WRITE_RATE_LIMITER.clone()),
            )
            .expect("Cannot create Google Artifact registry service"),
        ),
    )
    .expect("Cannot create Google Artifact Registry")
}

pub fn gcp_default_infra_config(
    context: &Context,
    logger: Box<dyn Logger>,
    metrics_registry: Box<dyn MetricsRegistry>,
) -> InfrastructureContext {
    let secrets = FuncTestsSecrets::new();

    Gke::docker_cr_engine(
        context,
        logger,
        metrics_registry,
        secrets
            .GCP_DEFAULT_REGION
            .expect("GCP_DEFAULT_REGION is not set in secrets")
            .as_str(),
        KubernetesKind::Gke,
        GCP_KUBERNETES_VERSION,
        &ClusterDomain::Default {
            cluster_id: context.cluster_short_id().to_string(),
        },
        None,
        i32::MAX, // NA on GKE due to autopilot
        i32::MAX, // NA on GKE due to autopilot
        CpuArchitecture::AMD64,
        EngineLocation::ClientSide,
    )
}

impl Cluster<Google, GkeOptions> for Gke {
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
    ) -> InfrastructureContext {
        // use Google Artifact registry
        let container_registry = Box::new(gcp_container_registry(context));

        // use local Docker
        let build_platform = Box::new(build_platform_local_docker(context));

        // use Google
        let cloud_provider: Box<dyn CloudProvider> =
            Self::cloud_provider(context, kubernetes_kind, localisation) as Box<dyn CloudProvider>;

        // use Qovery DNS provider
        let dns_provider: Box<dyn DnsProvider> = dns_provider_qoverydns(context, cluster_domain);

        // GKE cluster
        let cluster = get_environment_test_kubernetes(
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
        );

        InfrastructureContext::new(
            context.clone(),
            build_platform,
            container_registry,
            cloud_provider,
            dns_provider,
            cluster,
            metrics_registry,
            true,
        )
    }

    fn cloud_provider(context: &Context, _kubernetes_kind: KubernetesKind, localisation: &str) -> Box<Google> {
        let secrets = FuncTestsSecrets::new();
        let credentials = try_parse_json_credentials_from_str(
            secrets
                .GCP_CREDENTIALS
                .as_ref()
                .expect("GCP_CREDENTIALS is not set in secrets"),
        )
        .expect("Cannot parse GCP_CREDENTIALS");
        Box::new(Google::new(
            context.clone(),
            *context.cluster_long_id(),
            secrets
                .GCP_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("GCP_TEST_ORGANIZATION_ID is not set in secrets"),
            credentials,
            GcpRegion::from_str(localisation).expect("Unknown GCP region"),
            TerraformStateCredentials {
                access_key_id: secrets
                    .TERRAFORM_AWS_ACCESS_KEY_ID
                    .expect("TERRAFORM_AWS_ACCESS_KEY_ID is not set in secrets"),
                secret_access_key: secrets
                    .TERRAFORM_AWS_SECRET_ACCESS_KEY
                    .expect("TERRAFORM_AWS_SECRET_ACCESS_KEY is not set in secrets"),
                region: secrets.TERRAFORM_AWS_REGION.expect("TERRAFORM_AWS_REGION is not set"),
                s3_bucket: secrets.TERRAFORM_AWS_BUCKET.expect("TERRAFORM_AWS_BUCKET is not set"),
                dynamodb_table: secrets
                    .TERRAFORM_AWS_DYNAMODB_TABLE
                    .expect("TERRAFORM_AWS_DYNAMODB_TABLE is not set"),
            },
        ))
    }

    fn kubernetes_nodes(_min_nodes: i32, _max_nodes: i32, _cpu_archi: CpuArchitecture) -> Vec<NodeGroups> {
        Vec::with_capacity(0) // NA for GKE due to autopilot
    }

    fn kubernetes_cluster_options(
        secrets: FuncTestsSecrets,
        _cluster_id: Option<String>,
        engine_location: EngineLocation,
        vpc_network_mode: Option<VpcQoveryNetworkMode>,
    ) -> GkeOptions {
        let credentials = try_parse_json_credentials_from_str(
            secrets
                .GCP_CREDENTIALS
                .as_ref()
                .expect("GCP_CREDENTIALS is not set in secrets"),
        )
        .expect("Cannot parse GCP_CREDENTIALS");

        GkeOptions::new(
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
            credentials,
            VpcMode::Automatic {
                custom_cluster_ipv4_cidr_block: None,
                custom_services_ipv4_cidr_block: None,
            },
            vpc_network_mode,
            secrets
                .LETS_ENCRYPT_EMAIL_REPORT
                .expect("LETS_ENCRYPT_EMAIL_REPORT is not set in secrets"),
            Time::from_hms(5, 0, 0).expect("Cannot instantiate time"),
            Some(Time::from_hms(7, 0, 0).expect("Cannot instantiate time")),
        )
    }
}

pub fn try_parse_json_credentials_from_str(raw: &str) -> Result<JsonCredentials, String> {
    let credentials_io: JsonCredentialsIo =
        serde_json::from_str(raw).map_err(|e| format!("cannot parse raw credentials file: {e}"))?;
    JsonCredentials::try_from(credentials_io)
}

pub fn clean_environments(
    context: &Context,
    _environments: Vec<EnvironmentRequest>,
    secrets: FuncTestsSecrets,
    region: GcpRegion,
) -> Result<(), ContainerRegistryError> {
    let gcp_project_name = secrets
        .GCP_PROJECT_NAME
        .as_ref()
        .expect("GCP_PROJECT_NAME should be defined in secrets");
    let credentials = try_parse_json_credentials_from_str(
        secrets
            .GCP_CREDENTIALS
            .as_ref()
            .expect("GCP_CREDENTIALS is not set in secrets"),
    )
    .expect("Cannot parse GCP_CREDENTIALS");
    let service = ArtifactRegistryService::new(
        credentials.clone(),
        Some(GCP_ARTIFACT_REGISTRY_REPOSITORY_API_OBJECT_WRITE_RATE_LIMITER.clone()),
        Some(GCP_ARTIFACT_REGISTRY_IMAGE_API_OBJECT_WRITE_RATE_LIMITER.clone()),
    )
    .expect("Cannot initialize google artifact registry service");

    let _container_registry = GoogleArtifactRegistry::new(
        context.clone(),
        Uuid::new_v4(),
        "test",
        gcp_project_name,
        region,
        credentials,
        Arc::new(service),
    );

    // delete repository created in registry
    // TODO(benjaminch): delete repository

    Ok(())
}

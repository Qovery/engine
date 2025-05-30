use std::str::FromStr;
use std::sync::Arc;

use rand::Rng;
use tracing::error;
use uuid::Uuid;

use crate::helpers::common::{ActionableFeature, Cluster, ClusterDomain, NodeManager};
use crate::helpers::dns::dns_provider_qoverydns;
use crate::helpers::kubernetes::{
    KUBERNETES_MAX_NODES, KUBERNETES_MIN_NODES, TargetCluster, get_environment_test_kubernetes,
};
use crate::helpers::utilities::{FuncTestsSecrets, build_platform_local_docker, generate_id};
use qovery_engine::engine_task::qovery_api::FakeQoveryApi;
use qovery_engine::environment::models::scaleway::{ScwStorageType, ScwZone};
use qovery_engine::infrastructure::infrastructure_context::InfrastructureContext;
use qovery_engine::infrastructure::models::build_platform::Build;
use qovery_engine::infrastructure::models::cloud_provider::scaleway::Scaleway;
use qovery_engine::infrastructure::models::cloud_provider::scaleway::database_instance_type::ScwDatabaseInstanceType;
use qovery_engine::infrastructure::models::cloud_provider::{CloudProvider, TerraformStateCredentials};
use qovery_engine::infrastructure::models::container_registry::errors::ContainerRegistryError;
use qovery_engine::infrastructure::models::container_registry::scaleway_container_registry::ScalewayCR;
use qovery_engine::infrastructure::models::container_registry::{ContainerRegistry, InteractWithRegistry};
use qovery_engine::infrastructure::models::dns_provider::DnsProvider;
use qovery_engine::infrastructure::models::kubernetes::scaleway::kapsule::{KapsuleClusterType, KapsuleOptions};
use qovery_engine::infrastructure::models::kubernetes::{Kind as KubernetesKind, KubernetesVersion};
use qovery_engine::io_models::QoveryIdentifier;
use qovery_engine::io_models::context::Context;
use qovery_engine::io_models::engine_location::EngineLocation;
use qovery_engine::io_models::environment::EnvironmentRequest;
use qovery_engine::io_models::models::{CpuArchitecture, NodeGroups, StorageClass, VpcQoveryNetworkMode};
use qovery_engine::logger::Logger;
use qovery_engine::metrics_registry::MetricsRegistry;

pub const SCW_KUBERNETES_VERSION: KubernetesVersion = KubernetesVersion::V1_31 {
    prefix: None,
    patch: None,
    suffix: None,
};
pub const SCW_MANAGED_DATABASE_INSTANCE_TYPE: ScwDatabaseInstanceType = ScwDatabaseInstanceType::DB_DEV_S;
pub const SCW_MANAGED_DATABASE_DISK_TYPE: &str = "bssd";
pub const SCW_SELF_HOSTED_DATABASE_DISK_TYPE: &str = "scw-sbv-ssd-0";
pub const SCW_BUCKET_TTL_IN_SECONDS: u64 = 3600;
pub const SCW_RESOURCE_TTL_IN_SECONDS: u32 = 7200;

pub fn container_registry_scw(context: &Context) -> ScalewayCR {
    let secrets = FuncTestsSecrets::new();
    if secrets.SCALEWAY_ACCESS_KEY.is_none()
        || secrets.SCALEWAY_SECRET_KEY.is_none()
        || secrets.SCALEWAY_DEFAULT_PROJECT_ID.is_none()
    {
        error!(
            "Please check your Vault connectivity (token/address) or SCALEWAY_ACCESS_KEY/SCALEWAY_SECRET_KEY/SCALEWAY_DEFAULT_PROJECT_ID envrionment variables are set"
        );
        std::process::exit(1)
    }
    let random_id = generate_id();
    let scw_secret_key = secrets
        .SCALEWAY_SECRET_KEY
        .expect("SCALEWAY_SECRET_KEY is not set in secrets");
    let scw_default_project_id = secrets
        .SCALEWAY_DEFAULT_PROJECT_ID
        .expect("SCALEWAY_DEFAULT_PROJECT_ID is not set in secrets");

    ScalewayCR::new(
        context.clone(),
        Uuid::new_v4(),
        format!("default-registry-qovery-test-{random_id}").as_str(),
        scw_secret_key.as_str(),
        scw_default_project_id.as_str(),
        ScwZone::from_str(
            secrets
                .SCALEWAY_TEST_CLUSTER_REGION
                .expect("SCALEWAY_TEST_CLUSTER_REGION is not set")
                .as_str(),
        )
        .expect("Unknown SCW region")
        .region(),
    )
    .unwrap()
}

/// This method is dedicated to test services deployments
/// `node_manager` is set to default (no Karpenter)
/// `actionable_features` is empty
pub fn scw_infra_config(
    targeted_cluster: &TargetCluster,
    context: &Context,
    logger: Box<dyn Logger>,
    metrics_registry: Box<dyn MetricsRegistry>,
) -> InfrastructureContext {
    let secrets = FuncTestsSecrets::new();

    Scaleway::docker_cr_engine(
        context,
        logger,
        metrics_registry,
        secrets
            .SCALEWAY_TEST_CLUSTER_REGION
            .expect("SCALEWAY_TEST_CLUSTER_REGION is not set")
            .as_str(),
        KubernetesKind::ScwKapsule,
        SCW_KUBERNETES_VERSION,
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

impl Cluster<Scaleway, KapsuleOptions> for Scaleway {
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
        _cpu_archi: CpuArchitecture,
        engine_location: EngineLocation,
        kubeconfig: Option<String>,
        node_manager: NodeManager,
        actionable_features: Vec<ActionableFeature>,
    ) -> InfrastructureContext {
        // use Scaleway CR
        let container_registry = ContainerRegistry::ScalewayCr(container_registry_scw(context));

        // use LocalDocker
        let build_platform = Box::new(build_platform_local_docker(context));

        // use Scaleway
        let cloud_provider: Box<dyn CloudProvider> =
            Self::cloud_provider(context, kubernetes_kind, localisation) as Box<dyn CloudProvider>;
        let dns_provider: Box<dyn DnsProvider> = dns_provider_qoverydns(context, cluster_domain);

        let cluster = get_environment_test_kubernetes(
            context,
            cloud_provider.as_ref(),
            kubernetes_version,
            logger.clone(),
            localisation,
            vpc_network_mode,
            min_nodes,
            max_nodes,
            CpuArchitecture::AMD64,
            engine_location,
            StorageClass(ScwStorageType::SbvSsd.to_k8s_storage_class()),
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
            cluster,
            metrics_registry,
            true,
        )
    }

    fn cloud_provider(context: &Context, _kubernetes_kind: KubernetesKind, _localisation: &str) -> Box<Scaleway> {
        let secrets = FuncTestsSecrets::new();
        Box::new(Scaleway::new(
            *context.cluster_long_id(),
            secrets
                .SCALEWAY_ACCESS_KEY
                .expect("SCALEWAY_ACCESS_KEY is not set in secrets")
                .as_str(),
            secrets
                .SCALEWAY_SECRET_KEY
                .expect("SCALEWAY_SECRET_KEY is not set in secrets")
                .as_str(),
            secrets
                .SCALEWAY_DEFAULT_PROJECT_ID
                .expect("SCALEWAY_DEFAULT_PROJECT_ID is not set in secrets")
                .as_str(),
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

    fn kubernetes_nodes(min_nodes: i32, max_nodes: i32, cpu_archi: CpuArchitecture) -> Vec<NodeGroups> {
        let node_type = match cpu_archi {
            CpuArchitecture::AMD64 => "dev1-l".to_string(),
            CpuArchitecture::ARM64 => panic!("ARM64 not managed"),
        };

        // Note: Dev1M is a bit too small to handle engine + local docker, hence using Dev1L
        vec![
            NodeGroups::new("groupscw0".to_string(), min_nodes, max_nodes, node_type, 0, cpu_archi, None)
                .expect("Problem while setup SCW nodes"),
        ]
    }

    fn kubernetes_cluster_options(
        secrets: FuncTestsSecrets,
        _cluster_id: QoveryIdentifier,
        engine_location: EngineLocation,
        _vpc_network_mode: Option<VpcQoveryNetworkMode>,
    ) -> KapsuleOptions {
        KapsuleOptions::new(
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
            "admin".to_string(),
            "qovery".to_string(),
            engine_location,
            secrets
                .LETS_ENCRYPT_EMAIL_REPORT
                .expect("LETS_ENCRYPT_EMAIL_REPORT is not set in secrets"),
            KapsuleClusterType::Kapsule,
            None,
        )
    }
}

pub fn clean_environments(
    context: &Context,
    environments: Vec<EnvironmentRequest>,
    zone: ScwZone,
) -> Result<(), ContainerRegistryError> {
    let secrets = FuncTestsSecrets::new();
    let secret_token = secrets.SCALEWAY_SECRET_KEY.unwrap();
    let project_id = secrets.SCALEWAY_DEFAULT_PROJECT_ID.unwrap();

    let container_registry_client = ScalewayCR::new(
        context.clone(),
        Uuid::new_v4(),
        "test",
        secret_token.as_str(),
        project_id.as_str(),
        zone.region(),
    )?;

    // delete images created in registry
    let registry_url = container_registry_client.registry_info();
    for env in environments.iter() {
        for build in env
            .applications
            .iter()
            .map(|a| {
                a.to_build(
                    registry_url,
                    Arc::from(FakeQoveryApi {}),
                    vec![CpuArchitecture::AMD64, CpuArchitecture::ARM64],
                    &QoveryIdentifier::new(*context.cluster_long_id()),
                )
            })
            .collect::<Vec<Build>>()
        {
            let _ = container_registry_client.delete_image(&build.image);
        }
    }

    Ok(())
}

pub fn random_valid_registry_name() -> String {
    let mut rand_string: String = String::new();
    let mut rng = rand::rng();

    for x in 1..35 {
        if x % 4 == 0 {
            rand_string.push('-');
        } else {
            let char: char = rng.random_range(b'a'..=b'z') as char;
            rand_string.push(char);
        }
    }

    rand_string
}

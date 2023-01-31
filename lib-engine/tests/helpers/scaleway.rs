use std::str::FromStr;
use std::sync::Arc;

use const_format::formatcp;
use rand::Rng;
use tracing::error;
use uuid::Uuid;

use qovery_engine::build_platform::Build;
use qovery_engine::cloud_provider::aws::kubernetes::VpcQoveryNetworkMode;
use qovery_engine::cloud_provider::kubernetes::Kind as KubernetesKind;
use qovery_engine::cloud_provider::models::NodeGroups;
use qovery_engine::cloud_provider::qovery::EngineLocation;
use qovery_engine::cloud_provider::scaleway::kubernetes::KapsuleOptions;
use qovery_engine::cloud_provider::scaleway::Scaleway;
use qovery_engine::cloud_provider::{CloudProvider, TerraformStateCredentials};
use qovery_engine::container_registry::errors::ContainerRegistryError;
use qovery_engine::container_registry::scaleway_container_registry::ScalewayCR;
use qovery_engine::container_registry::ContainerRegistry;
use qovery_engine::dns_provider::DnsProvider;
use qovery_engine::engine::InfrastructureContext;
use qovery_engine::engine_task::core_service_api::FakeCoreServiceApi;
use qovery_engine::io_models::context::Context;
use qovery_engine::io_models::environment::EnvironmentRequest;
use qovery_engine::logger::Logger;
use qovery_engine::models::scaleway::ScwZone;

use crate::helpers::common::{Cluster, ClusterDomain};
use crate::helpers::dns::dns_provider_qoverydns;
use crate::helpers::kubernetes::{get_environment_test_kubernetes, KUBERNETES_MAX_NODES, KUBERNETES_MIN_NODES};
use crate::helpers::utilities::{build_platform_local_docker, generate_id, FuncTestsSecrets};

pub const SCW_KUBERNETES_MAJOR_VERSION: u8 = 1;
pub const SCW_KUBERNETES_MINOR_VERSION: u8 = 22;
pub const SCW_KUBERNETES_VERSION: &str = formatcp!("{}.{}", SCW_KUBERNETES_MAJOR_VERSION, SCW_KUBERNETES_MINOR_VERSION);
pub const SCW_MANAGED_DATABASE_INSTANCE_TYPE: &str = "db-dev-s";
pub const SCW_MANAGED_DATABASE_DISK_TYPE: &str = "bssd";
pub const SCW_SELF_HOSTED_DATABASE_INSTANCE_TYPE: &str = "";
pub const SCW_SELF_HOSTED_DATABASE_DISK_TYPE: &str = "scw-sbv-ssd-0";
pub const SCW_BUCKET_TTL_IN_SECONDS: i32 = 3600;

pub fn container_registry_scw(context: &Context) -> ScalewayCR {
    let secrets = FuncTestsSecrets::new();
    if secrets.SCALEWAY_ACCESS_KEY.is_none()
        || secrets.SCALEWAY_SECRET_KEY.is_none()
        || secrets.SCALEWAY_DEFAULT_PROJECT_ID.is_none()
    {
        error!("Please check your Vault connectivity (token/address) or SCALEWAY_ACCESS_KEY/SCALEWAY_SECRET_KEY/SCALEWAY_DEFAULT_PROJECT_ID envrionment variables are set");
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
        format!("default-registry-qovery-test-{}", random_id).as_str(),
        Uuid::new_v4(),
        format!("default-registry-qovery-test-{}", random_id).as_str(),
        scw_secret_key.as_str(),
        scw_default_project_id.as_str(),
        ScwZone::from_str(
            secrets
                .SCALEWAY_TEST_CLUSTER_REGION
                .expect("SCALEWAY_TEST_CLUSTER_REGION is not set")
                .as_str(),
        )
        .expect("Unknown SCW region"),
    )
    .unwrap()
}

pub fn scw_default_infra_config(context: &Context, logger: Box<dyn Logger>) -> InfrastructureContext {
    let secrets = FuncTestsSecrets::new();

    Scaleway::docker_cr_engine(
        context,
        logger,
        secrets
            .SCALEWAY_TEST_CLUSTER_REGION
            .expect("SCALEWAY_TEST_CLUSTER_REGION is not set")
            .as_str(),
        KubernetesKind::ScwKapsule,
        SCW_KUBERNETES_VERSION.to_string(),
        &ClusterDomain::Default {
            cluster_id: context.cluster_short_id().to_string(),
        },
        None,
        KUBERNETES_MIN_NODES,
        KUBERNETES_MAX_NODES,
        EngineLocation::ClientSide,
    )
}

impl Cluster<Scaleway, KapsuleOptions> for Scaleway {
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
    ) -> InfrastructureContext {
        // use Scaleway CR
        let container_registry = Box::new(container_registry_scw(context));

        // use LocalDocker
        let build_platform = Box::new(build_platform_local_docker(context));

        // use Scaleway
        let cloud_provider: Arc<Box<dyn CloudProvider>> =
            Arc::new(Self::cloud_provider(context, kubernetes_kind, localisation));
        let dns_provider: Arc<Box<dyn DnsProvider>> = Arc::new(dns_provider_qoverydns(context, cluster_domain));

        let cluster = get_environment_test_kubernetes(
            context,
            cloud_provider.clone(),
            kubernetes_version.as_str(),
            dns_provider.clone(),
            logger.clone(),
            localisation,
            vpc_network_mode,
            min_nodes,
            max_nodes,
            engine_location,
        );

        InfrastructureContext::new(
            context.clone(),
            build_platform,
            container_registry,
            cloud_provider,
            dns_provider,
            cluster,
        )
    }

    fn cloud_provider(context: &Context, _kubernetes_kind: KubernetesKind, _localisation: &str) -> Box<Scaleway> {
        let secrets = FuncTestsSecrets::new();
        Box::new(Scaleway::new(
            context.clone(),
            *context.cluster_long_id(),
            secrets
                .SCALEWAY_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("SCALEWAY_TEST_ORGANIZATION_ID is not set")
                .as_str(),
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
            secrets
                .SCALEWAY_DEFAULT_REGION
                .expect("SCALEWAY_DEFAULT_REGION is not set in secrets")
                .as_str(),
            TerraformStateCredentials {
                access_key_id: secrets
                    .TERRAFORM_AWS_ACCESS_KEY_ID
                    .expect("TERRAFORM_AWS_ACCESS_KEY_ID is not set in secrets"),
                secret_access_key: secrets
                    .TERRAFORM_AWS_SECRET_ACCESS_KEY
                    .expect("TERRAFORM_AWS_SECRET_ACCESS_KEY is not set in secrets"),
                region: "eu-west-3".to_string(),
            },
        ))
    }

    fn kubernetes_nodes(min_nodes: i32, max_nodes: i32) -> Vec<NodeGroups> {
        // Note: Dev1M is a bit too small to handle engine + local docker, hence using Dev1L
        vec![
            NodeGroups::new("groupscw0".to_string(), min_nodes, max_nodes, "dev1-l".to_string(), 0)
                .expect("Problem while setup SCW nodes"),
        ]
    }

    fn kubernetes_cluster_options(
        secrets: FuncTestsSecrets,
        _cluster_id: Option<String>,
        engine_location: EngineLocation,
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
            secrets
                .QOVERY_AGENT_CONTROLLER_TOKEN
                .expect("QOVERY_AGENT_CONTROLLER_TOKEN is not set in secrets"),
            engine_location,
            secrets
                .QOVERY_ENGINE_CONTROLLER_TOKEN
                .expect("QOVERY_ENGINE_CONTROLLER_TOKEN is not set in secrets"),
            secrets
                .SCALEWAY_DEFAULT_PROJECT_ID
                .expect("SCALEWAY_DEFAULT_PROJECT_ID is not set in secrets"),
            secrets
                .SCALEWAY_ACCESS_KEY
                .expect("SCALEWAY_ACCESS_KEY is not set in secrets"),
            secrets
                .SCALEWAY_SECRET_KEY
                .expect("SCALEWAY_SECRET_KEY is not set in secrets"),
            secrets
                .LETS_ENCRYPT_EMAIL_REPORT
                .expect("LETS_ENCRYPT_EMAIL_REPORT is not set in secrets"),
        )
    }
}

pub fn clean_environments(
    context: &Context,
    environments: Vec<EnvironmentRequest>,
    secrets: FuncTestsSecrets,
    zone: ScwZone,
) -> Result<(), ContainerRegistryError> {
    let secret_token = secrets.SCALEWAY_SECRET_KEY.unwrap();
    let project_id = secrets.SCALEWAY_DEFAULT_PROJECT_ID.unwrap();

    let container_registry_client = ScalewayCR::new(
        context.clone(),
        "test",
        Uuid::new_v4(),
        "test",
        secret_token.as_str(),
        project_id.as_str(),
        zone,
    )?;

    // delete images created in registry
    let registry_url = container_registry_client.registry_info();
    for env in environments.iter() {
        for build in env
            .applications
            .iter()
            .map(|a| a.to_build(registry_url, Arc::new(Box::new(FakeCoreServiceApi {}))))
            .collect::<Vec<Build>>()
        {
            let _ = container_registry_client.delete_image(&build.image);
        }
    }

    Ok(())
}

pub fn random_valid_registry_name() -> String {
    let mut rand_string: String = String::new();
    let mut rng = rand::thread_rng();

    for x in 1..35 {
        if x % 4 == 0 {
            rand_string.push('-');
        } else {
            let char: char = rng.gen_range(b'a'..=b'z') as char;
            rand_string.push(char);
        }
    }

    rand_string
}

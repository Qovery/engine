use const_format::formatcp;
use qovery_engine::build_platform::Image;
use qovery_engine::cloud_provider::scaleway::application::ScwZone;
use qovery_engine::cloud_provider::scaleway::kubernetes::KapsuleOptions;
use qovery_engine::cloud_provider::scaleway::Scaleway;
use qovery_engine::cloud_provider::TerraformStateCredentials;
use qovery_engine::container_registry::scaleway_container_registry::ScalewayCR;
use qovery_engine::engine::Engine;
use qovery_engine::error::EngineError;
use qovery_engine::models::{Context, Environment};
use qovery_engine::object_storage::scaleway_object_storage::{BucketDeleteStrategy, ScalewayOS};

use crate::cloudflare::dns_provider_cloudflare;
use crate::utilities::{build_platform_local_docker, generate_id, FuncTestsSecrets};

use crate::common::{Cluster, ClusterDomain};
use qovery_engine::cloud_provider::models::NodeGroups;
use qovery_engine::cloud_provider::qovery::EngineLocation;
use qovery_engine::logger::Logger;
use tracing::error;

pub const SCW_TEST_ZONE: ScwZone = ScwZone::Paris2;
pub const SCW_KUBERNETES_MAJOR_VERSION: u8 = 1;
pub const SCW_KUBERNETES_MINOR_VERSION: u8 = 19;
pub const SCW_KUBERNETES_VERSION: &'static str =
    formatcp!("{}.{}", SCW_KUBERNETES_MAJOR_VERSION, SCW_KUBERNETES_MINOR_VERSION);
pub const SCW_MANAGED_DATABASE_INSTANCE_TYPE: &str = "db-dev-s";
pub const SCW_MANAGED_DATABASE_DISK_TYPE: &str = "bssd";
pub const SCW_SELF_HOSTED_DATABASE_INSTANCE_TYPE: &str = "";
pub const SCW_SELF_HOSTED_DATABASE_DISK_TYPE: &str = "scw-sbv-ssd-0";
pub const SCW_RESOURCE_TTL_IN_SECONDS: u32 = 7200;

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
        format!("default-registry-qovery-test-{}", random_id.clone()).as_str(),
        format!("default-registry-qovery-test-{}", random_id.clone()).as_str(),
        scw_secret_key.as_str(),
        scw_default_project_id.as_str(),
        SCW_TEST_ZONE,
    )
}

impl Cluster<Scaleway, KapsuleOptions> for Scaleway {
    fn docker_cr_engine(context: &Context, logger: Box<dyn Logger>) -> Engine {
        // use Scaleway CR
        let container_registry = Box::new(container_registry_scw(context));

        // use LocalDocker
        let build_platform = Box::new(build_platform_local_docker(context));

        // use Scaleway
        let cloud_provider = Scaleway::cloud_provider(context);

        let dns_provider = Box::new(dns_provider_cloudflare(context, ClusterDomain::Default));

        Engine::new(
            context.clone(),
            build_platform,
            container_registry,
            cloud_provider,
            dns_provider,
            logger,
        )
    }

    fn cloud_provider(context: &Context) -> Box<Scaleway> {
        let secrets = FuncTestsSecrets::new();
        let cluster_id = secrets
            .SCALEWAY_TEST_CLUSTER_ID
            .expect("SCALEWAY_TEST_CLUSTER_ID is not set");
        Box::new(Scaleway::new(
            context.clone(),
            cluster_id.as_str(),
            secrets
                .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set")
                .as_str(),
            uuid::Uuid::new_v4(),
            format!("qovery-{}", cluster_id).as_str(),
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
                region: "eu-west-3".to_string(),
            },
        ))
    }

    fn kubernetes_nodes() -> Vec<NodeGroups> {
        // Note: Dev1M is a bit too small to handle engine + local docker, hence using Dev1L
        vec![NodeGroups::new("groupscw0".to_string(), 5, 10, "dev1-l".to_string(), 0)
            .expect("Problem while setup SCW nodes")]
    }

    fn kubernetes_cluster_options(secrets: FuncTestsSecrets, _cluster_name: Option<String>) -> KapsuleOptions {
        KapsuleOptions::new(
            secrets.QOVERY_API_URL.expect("QOVERY_API_URL is not set in secrets"),
            secrets.QOVERY_GRPC_URL.expect("QOVERY_GRPC_URL is not set in secrets"),
            secrets
                .QOVERY_CLUSTER_SECRET_TOKEN
                .expect("QOVERY_CLUSTER_SECRET_TOKEN is not set in secrets"),
            secrets.QOVERY_NATS_URL.expect("QOVERY_NATS_URL is not set in secrets"),
            secrets
                .QOVERY_NATS_USERNAME
                .expect("QOVERY_NATS_USERNAME is not set in secrets"),
            secrets
                .QOVERY_NATS_PASSWORD
                .expect("QOVERY_NATS_PASSWORD is not set in secrets"),
            secrets.QOVERY_SSH_USER.expect("QOVERY_SSH_USER is not set in secrets"),
            "admin".to_string(),
            "qovery".to_string(),
            secrets
                .QOVERY_AGENT_CONTROLLER_TOKEN
                .expect("QOVERY_AGENT_CONTROLLER_TOKEN is not set in secrets"),
            EngineLocation::ClientSide,
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

pub fn scw_object_storage(context: Context, region: ScwZone) -> ScalewayOS {
    let secrets = FuncTestsSecrets::new();
    let random_id = generate_id();

    ScalewayOS::new(
        context,
        format!("qovery-test-object-storage-{}", random_id.clone()),
        format!("Qovery Test Object-Storage {}", random_id),
        secrets
            .SCALEWAY_ACCESS_KEY
            .expect("SCALEWAY_ACCESS_KEY is not set in secrets"),
        secrets
            .SCALEWAY_SECRET_KEY
            .expect("SCALEWAY_SECRET_KEY is not set in secrets"),
        region,
        BucketDeleteStrategy::Empty, // do not delete bucket due to deletion 24h delay
        false,
        Some(SCW_RESOURCE_TTL_IN_SECONDS),
    )
}

pub fn clean_environments(
    context: &Context,
    environments: Vec<Environment>,
    secrets: FuncTestsSecrets,
    zone: ScwZone,
) -> Result<(), EngineError> {
    let secret_token = secrets.SCALEWAY_SECRET_KEY.unwrap();
    let project_id = secrets.SCALEWAY_DEFAULT_PROJECT_ID.unwrap();

    let container_registry_client = ScalewayCR::new(
        context.clone(),
        "test",
        "test",
        secret_token.as_str(),
        project_id.as_str(),
        zone,
    );

    // delete images created in registry
    for env in environments.iter() {
        for image in env.applications.iter().map(|a| a.to_image()).collect::<Vec<Image>>() {
            if let Err(e) = container_registry_client.delete_image(&image) {
                return Err(e);
            }
        }
    }

    Ok(())
}

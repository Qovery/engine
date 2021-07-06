use qovery_engine::cloud_provider::scaleway::Scaleway;
use qovery_engine::cloud_provider::scaleway::application::Region;
use qovery_engine::cloud_provider::TerraformStateCredentials;
use qovery_engine::container_registry::scaleway_cr::ScalewayCR;
use qovery_engine::engine::Engine;
use qovery_engine::models::Context;

use crate::cloudflare::dns_provider_cloudflare;
use crate::utilities::{build_platform_local_docker, generate_id, FuncTestsSecrets};

use tracing::error;

const SCW_TEST_CLUSTER_NAME: &str = "Qovery test cluster";
const SCW_TEST_CLUSTER_ID: &str = "qovery-test-cluster";
const SCW_TEST_REGION: Region = Region::Paris;

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
    let scw_secret_key = secrets.SCALEWAY_SECRET_KEY.unwrap_or("undefined".to_string());
    let scw_default_project_id = secrets.SCALEWAY_DEFAULT_PROJECT_ID.unwrap_or("undefined".to_string());

    ScalewayCR::new(
        context.clone(),
        format!("default-ecr-registry-qovery-test-{}", random_id.clone()),
        format!("default-ecr-registry-qovery-test-{}", random_id.clone()),
        scw_secret_key,
        scw_default_project_id,
        SCW_TEST_REGION,
    )
}

pub fn cloud_provider_scaleway(context: &Context) -> Scaleway {
    let secrets = FuncTestsSecrets::new();

    Scaleway::new(
        context.clone(),
        SCW_TEST_CLUSTER_ID,
        secrets.SCALEWAY_DEFAULT_PROJECT_ID.unwrap_or("undefined".to_string()).as_str(),
        SCW_TEST_CLUSTER_NAME,
        secrets.SCALEWAY_ACCESS_KEY.unwrap_or("undefined".to_string()).as_str(),
        secrets.SCALEWAY_SECRET_KEY.unwrap_or("undefined".to_string()).as_str(),
        TerraformStateCredentials {
            access_key_id: secrets.TERRAFORM_AWS_ACCESS_KEY_ID.unwrap(),
            secret_access_key: secrets.TERRAFORM_AWS_SECRET_ACCESS_KEY.unwrap(),
            region: "eu-west-3".to_string(),
        },
    )
}

pub fn docker_scw_cr_engine(context: &Context) -> Engine {
    // use Scaleway CR
    let container_registry = Box::new(container_registry_scw(context));

    // use LocalDocker
    let build_platform = Box::new(build_platform_local_docker(context));

    // use Scaleway
    let cloud_provider = Box::new(cloud_provider_scaleway(context));

    let dns_provider = Box::new(dns_provider_cloudflare(context));

    Engine::new(
        context.clone(),
        build_platform,
        container_registry,
        cloud_provider,
        dns_provider,
    )
}
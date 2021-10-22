use qovery_engine::build_platform::Image;
use qovery_engine::cloud_provider::digitalocean::kubernetes::DoksOptions;
use qovery_engine::cloud_provider::digitalocean::kubernetes::DOKS;
use qovery_engine::cloud_provider::digitalocean::network::vpc::VpcInitKind;
use qovery_engine::cloud_provider::digitalocean::DO;
use qovery_engine::cloud_provider::models::NodeGroups;
use qovery_engine::cloud_provider::TerraformStateCredentials;
use qovery_engine::container_registry::docr::DOCR;
use qovery_engine::dns_provider::DnsProvider;
use qovery_engine::engine::Engine;
use qovery_engine::error::EngineError;
use qovery_engine::models::{Context, Environment, EnvironmentAction};
use qovery_engine::transaction::{DeploymentOption, TransactionResult};

use crate::cloudflare::dns_provider_cloudflare;
use crate::utilities::{build_platform_local_docker, FuncTestsSecrets};
use qovery_engine::cloud_provider::digitalocean::application::Region;
use qovery_engine::cloud_provider::qovery::EngineLocation;

pub const DO_QOVERY_ORGANIZATION_ID: &str = "z3bc003d2";
pub const DO_KUBERNETES_VERSION: &str = "1.19";
pub const DOCR_ID: &str = "gu9ep7t68htdu78l";
pub const DO_KUBE_TEST_CLUSTER_ID: &str = "z2a1b27a3";
pub const DO_KUBE_TEST_CLUSTER_NAME: &str = "qovery-z2a1b27a3";
pub const DO_TEST_REGION: Region = Region::NewYorkCity3;
pub const DO_MANAGED_DATABASE_INSTANCE_TYPE: &str = "not-used";
pub const DO_MANAGED_DATABASE_DISK_TYPE: &str = "not-used";
pub const DO_SELF_HOSTED_DATABASE_INSTANCE_TYPE: &str = "not-used";
pub const DO_SELF_HOSTED_DATABASE_DISK_TYPE: &str = "do-sbv-ssd-0";

pub fn container_registry_digital_ocean(context: &Context) -> DOCR {
    let secrets = FuncTestsSecrets::new();
    DOCR::new(
        context.clone(),
        DOCR_ID,
        "default-docr-registry-qovery-do-test",
        secrets.DIGITAL_OCEAN_TOKEN.unwrap().as_str(),
    )
}

pub fn docker_cr_do_engine(context: &Context) -> Engine {
    // use DigitalOcean Container Registry
    let container_registry = Box::new(container_registry_digital_ocean(context));
    // use LocalDocker
    let build_platform = Box::new(build_platform_local_docker(context));
    // use Digital Ocean
    let cloud_provider = Box::new(cloud_provider_digitalocean(context));

    let dns_provider = Box::new(dns_provider_cloudflare(&context));

    Engine::new(
        context.clone(),
        build_platform,
        container_registry,
        cloud_provider,
        dns_provider,
    )
}

pub fn do_kubernetes_ks<'a>(
    context: &Context,
    cloud_provider: &'a DO,
    dns_provider: &'a dyn DnsProvider,
    nodes_groups: Vec<NodeGroups>,
    region: Region,
) -> DOKS<'a> {
    let secrets = FuncTestsSecrets::new();
    DOKS::<'a>::new(
        context.clone(),
        DO_KUBE_TEST_CLUSTER_ID.to_string(),
        uuid::Uuid::new_v4(),
        DO_KUBE_TEST_CLUSTER_NAME.to_string(),
        DO_KUBERNETES_VERSION.to_string(),
        region,
        cloud_provider,
        dns_provider,
        nodes_groups,
        do_kubernetes_cluster_options(secrets, DO_KUBE_TEST_CLUSTER_ID.to_string()),
    )
    .unwrap()
}

pub fn do_kubernetes_nodes() -> Vec<NodeGroups> {
    vec![
        NodeGroups::new("groupdoks0".to_string(), 5, 10, "s-4vcpu-8gb".to_string())
            .expect("Problem while setup DOKS nodes"),
    ]
}

pub fn cloud_provider_digitalocean(context: &Context) -> DO {
    let secrets = FuncTestsSecrets::new();
    DO::new(
        context.clone(),
        DO_KUBE_TEST_CLUSTER_ID,
        DO_QOVERY_ORGANIZATION_ID,
        uuid::Uuid::new_v4(),
        secrets.DIGITAL_OCEAN_TOKEN.unwrap().as_str(),
        secrets.DIGITAL_OCEAN_SPACES_ACCESS_ID.unwrap().as_str(),
        secrets.DIGITAL_OCEAN_SPACES_SECRET_ID.unwrap().as_str(),
        DO_KUBE_TEST_CLUSTER_NAME,
        TerraformStateCredentials {
            access_key_id: secrets.TERRAFORM_AWS_ACCESS_KEY_ID.unwrap(),
            secret_access_key: secrets.TERRAFORM_AWS_SECRET_ACCESS_KEY.unwrap(),
            region: secrets.TERRAFORM_AWS_REGION.unwrap(),
        },
    )
}

pub fn do_kubernetes_cluster_options(secrets: FuncTestsSecrets, cluster_name: String) -> DoksOptions {
    DoksOptions {
        vpc_cidr_block: "should-not-bet-set".to_string(), // vpc_cidr_set to autodetect will fil this empty string
        vpc_cidr_set: VpcInitKind::Autodetect,
        vpc_name: cluster_name,
        qovery_api_url: secrets.QOVERY_API_URL.unwrap(),
        qovery_grpc_url: secrets.QOVERY_GRPC_URL.unwrap(),
        qovery_cluster_secret_token: secrets.QOVERY_CLUSTER_SECRET_TOKEN.unwrap(),
        qovery_engine_location: Some(EngineLocation::ClientSide),
        engine_version_controller_token: secrets.QOVERY_ENGINE_CONTROLLER_TOKEN.unwrap(),
        agent_version_controller_token: secrets.QOVERY_AGENT_CONTROLLER_TOKEN.unwrap(),
        grafana_admin_user: "admin".to_string(),
        grafana_admin_password: "qovery".to_string(),
        discord_api_key: secrets.DISCORD_API_URL.unwrap(),
        qovery_nats_url: secrets.QOVERY_NATS_URL.unwrap(),
        qovery_nats_user: secrets.QOVERY_NATS_USERNAME.unwrap(),
        qovery_nats_password: secrets.QOVERY_NATS_PASSWORD.unwrap(),
        qovery_ssh_key: secrets.QOVERY_SSH_USER.unwrap(),
        tls_email_report: secrets.LETS_ENCRYPT_EMAIL_REPORT.unwrap(),
    }
}

pub fn deploy_environment(
    context: &Context,
    environment_action: EnvironmentAction,
    region: Region,
) -> TransactionResult {
    let engine = docker_cr_do_engine(context);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let cp = cloud_provider_digitalocean(context);
    let nodes = do_kubernetes_nodes();
    let dns_provider = dns_provider_cloudflare(context);
    let doks = do_kubernetes_ks(context, &cp, &dns_provider, nodes, region);

    let _ = tx.deploy_environment_with_options(
        &doks,
        &environment_action,
        DeploymentOption {
            force_build: true,
            force_push: true,
        },
    );

    tx.commit()
}

pub fn delete_environment(
    context: &Context,
    environment_action: EnvironmentAction,
    region: Region,
) -> TransactionResult {
    let engine = docker_cr_do_engine(context);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let cp = cloud_provider_digitalocean(context);
    let nodes = do_kubernetes_nodes();
    let dns_provider = dns_provider_cloudflare(&context);
    let doks = do_kubernetes_ks(context, &cp, &dns_provider, nodes, region);

    let _ = tx.delete_environment(&doks, &environment_action);

    tx.commit()
}

pub fn pause_environment(
    context: &Context,
    environment_action: EnvironmentAction,
    region: Region,
) -> TransactionResult {
    let engine = docker_cr_do_engine(context);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let cp = cloud_provider_digitalocean(context);
    let nodes = do_kubernetes_nodes();
    let dns_provider = dns_provider_cloudflare(&context);
    let doks = do_kubernetes_ks(context, &cp, &dns_provider, nodes, region);

    let _ = tx.pause_environment(&doks, &environment_action);

    tx.commit()
}

pub fn clean_environments(
    context: &Context,
    environments: Vec<Environment>,
    secrets: FuncTestsSecrets,
    _region: Region,
) -> Result<(), EngineError> {
    let do_cr = DOCR::new(
        context.clone(),
        "test",
        "test",
        secrets
            .DIGITAL_OCEAN_TOKEN
            .as_ref()
            .expect("DIGITAL_OCEAN_TOKEN is not set in secrets"),
    );

    // delete images created in registry
    for env in environments.iter() {
        for image in env.applications.iter().map(|a| a.to_image()).collect::<Vec<Image>>() {
            if let Err(e) = do_cr.delete_image(&image) {
                return Err(e);
            }
        }
    }

    Ok(())
}

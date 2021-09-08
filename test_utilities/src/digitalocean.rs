use qovery_engine::cloud_provider::digitalocean::kubernetes::node::Node;
use qovery_engine::cloud_provider::digitalocean::kubernetes::DoksOptions;
use qovery_engine::cloud_provider::digitalocean::kubernetes::DOKS;
use qovery_engine::cloud_provider::digitalocean::network::vpc::VpcInitKind;
use qovery_engine::cloud_provider::digitalocean::DO;
use qovery_engine::cloud_provider::TerraformStateCredentials;
use qovery_engine::container_registry::docr::DOCR;
use qovery_engine::dns_provider::DnsProvider;
use qovery_engine::engine::Engine;
use qovery_engine::models::Context;

use crate::cloudflare::dns_provider_cloudflare;
use crate::utilities::{build_platform_local_docker, FuncTestsSecrets};
use qovery_engine::cloud_provider::digitalocean::application::Region;

pub const DO_QOVERY_ORGANIZATION_ID: &str = "a8nb94c7fwxzr2ja";
pub const DO_KUBERNETES_VERSION: &str = "1.19";
pub const DOCR_ID: &str = "gu9ep7t68htdu78l";
pub const DOKS_KUBE_TEST_CLUSTER_ID: &str = "gqgyb7zy4ykwumak";
pub const DOKS_KUBE_TEST_CLUSTER_NAME: &str = "QoveryDigitalOceanTest";

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

    let dns_provider = Box::new(dns_provider_cloudflare(context));

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
    nodes: Vec<Node>,
) -> DOKS<'a> {
    let secrets = FuncTestsSecrets::new();
    DOKS::<'a>::new(
        context.clone(),
        DOKS_KUBE_TEST_CLUSTER_ID.to_string(),
        DOKS_KUBE_TEST_CLUSTER_NAME.to_string(),
        DO_KUBERNETES_VERSION.to_string(),
        Region::Frankfurt,
        cloud_provider,
        dns_provider,
        nodes,
        do_kubernetes_cluster_options(secrets, DOKS_KUBE_TEST_CLUSTER_ID.to_string()),
    )
}

pub fn do_kubernetes_nodes() -> Vec<Node> {
    vec![
        Node::new_with_cpu_and_mem(4, 8),
        Node::new_with_cpu_and_mem(4, 8),
        Node::new_with_cpu_and_mem(4, 8),
        Node::new_with_cpu_and_mem(4, 8),
        Node::new_with_cpu_and_mem(4, 8),
        Node::new_with_cpu_and_mem(4, 8),
        Node::new_with_cpu_and_mem(4, 8),
        Node::new_with_cpu_and_mem(4, 8),
        Node::new_with_cpu_and_mem(4, 8),
        Node::new_with_cpu_and_mem(4, 8),
    ]
}

pub fn cloud_provider_digitalocean(context: &Context) -> DO {
    let secrets = FuncTestsSecrets::new();
    DO::new(
        context.clone(),
        DOKS_KUBE_TEST_CLUSTER_ID,
        DO_QOVERY_ORGANIZATION_ID,
        secrets.DIGITAL_OCEAN_TOKEN.unwrap().as_str(),
        secrets.DIGITAL_OCEAN_SPACES_ACCESS_ID.unwrap().as_str(),
        secrets.DIGITAL_OCEAN_SPACES_SECRET_ID.unwrap().as_str(),
        DOKS_KUBE_TEST_CLUSTER_NAME,
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

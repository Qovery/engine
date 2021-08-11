use std::fs::File;

use reqwest::StatusCode;

use qovery_engine::cloud_provider::digitalocean::kubernetes::node::Node;
use qovery_engine::cloud_provider::digitalocean::kubernetes::Options;
use qovery_engine::cloud_provider::digitalocean::kubernetes::DOKS;
use qovery_engine::cloud_provider::digitalocean::models::cluster::Cluster;
use qovery_engine::cloud_provider::digitalocean::network::vpc::VpcInitKind;
use qovery_engine::cloud_provider::digitalocean::DO;
use qovery_engine::cloud_provider::TerraformStateCredentials;
use qovery_engine::container_registry::docr::DOCR;
use qovery_engine::dns_provider::DnsProvider;
use qovery_engine::engine::Engine;
use qovery_engine::models::Context;

use crate::cloudflare::dns_provider_cloudflare;
use crate::utilities::{build_platform_local_docker, generate_cluster_id, FuncTestsSecrets};
use qovery_engine::cloud_provider::digitalocean::application::Region;

pub const ORGANIZATION_ID: &str = "a8nb94c7fwxzr2ja";
pub const DO_KUBERNETES_VERSION: &str = "1.18.10-do.3";
pub const DIGITAL_OCEAN_URL: &str = "https://api.digitalocean.com/v2/";
pub const DOCR_ID: &str = "gu9ep7t68htdu78l";
pub const DOKS_CLUSTER_ID: &str = "gqgyb7zy4ykwumak";
pub const DOKS_CLUSTER_NAME: &str = "QoveryDigitalOceanTest";

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
    let file = File::open("tests/assets/do-options.json").expect("file not found");
    let options_values = serde_json::from_reader(file).expect("JSON was not well-formatted");
    DOKS::<'a>::new(
        context.clone(),
        DOKS_CLUSTER_ID,
        DOKS_CLUSTER_NAME,
        DO_KUBERNETES_VERSION,
        Region::Frankfurt,
        cloud_provider,
        dns_provider,
        options_values,
        nodes,
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
        "test",
        ORGANIZATION_ID,
        secrets.DIGITAL_OCEAN_TOKEN.unwrap().as_str(),
        secrets.DIGITAL_OCEAN_SPACES_ACCESS_ID.unwrap().as_str(),
        secrets.DIGITAL_OCEAN_SPACES_SECRET_ID.unwrap().as_str(),
        DOKS_CLUSTER_NAME,
        TerraformStateCredentials {
            access_key_id: secrets.TERRAFORM_AWS_ACCESS_KEY_ID.unwrap(),
            secret_access_key: secrets.TERRAFORM_AWS_SECRET_ACCESS_KEY.unwrap(),
            region: "eu-west-3".to_string(),
        },
    )
}

pub fn do_kubernetes_cluster_options(secrets: FuncTestsSecrets) -> Options {
    Options {
        vpc_cidr_block: "".to_string(),
        vpc_cidr_set: VpcInitKind::Autodetect,
        vpc_name: format!(
            "vpc-test-{}",
            generate_cluster_id(&secrets.DIGITAL_OCEAN_DEFAULT_REGION.unwrap())
        ),
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

pub fn get_kube_cluster_name_from_uuid(uuid: &str) -> String {
    let secrets = FuncTestsSecrets::new();
    let headers = qovery_engine::utilities::get_header_with_bearer(secrets.DIGITAL_OCEAN_TOKEN.unwrap().as_str());
    let path = format!("https://api.digitalocean.com/v2/kubernetes/clusters/{}", uuid);
    let res = reqwest::blocking::Client::new()
        .get(path.as_str())
        .headers(headers)
        .send();
    match res {
        Ok(response) => match response.status() {
            StatusCode::OK => {
                let content = response.text().unwrap();
                let res_cluster = serde_json::from_str::<Cluster>(&content);
                match res_cluster {
                    Ok(cluster) => return cluster.kubernetes_cluster.name.clone(),
                    Err(e) => panic!("{}", e),
                }
            }
            _ => return String::from(""),
        },
        Err(_) => return String::from(""),
    }
}

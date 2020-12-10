use digitalocean::DigitalOcean;

use qovery_engine::cloud_provider::digitalocean::DO;
use qovery_engine::container_registry::docr;
use qovery_engine::container_registry::docr::{get_header_with_bearer, DOCR};
use qovery_engine::dns_provider::cloudflare::Cloudflare;
use qovery_engine::engine::Engine;
use qovery_engine::models::Context;

use crate::aws::{terraform_aws_access_key_id, terraform_aws_secret_access_key};
use crate::cloudflare::dns_provider_cloudflare;
use crate::utilities::build_platform_local_docker;
use qovery_engine::cloud_provider::digitalocean::api_structs::clusters::Cluster;
use qovery_engine::cloud_provider::digitalocean::kubernetes::node::Node;
use qovery_engine::cloud_provider::digitalocean::kubernetes::DOKS;
use qovery_engine::cloud_provider::TerraformStateCredentials;
use qovery_engine::dns_provider::DnsProvider;
use reqwest::StatusCode;
use std::fs::File;

pub const ORGANIZATION_ID: &str = "a8nb94c7fwxzr2ja";
pub const DO_KUBERNETES_VERSION: &str = "1.18.10-do.2";
pub const DIGITAL_OCEAN_URL: &str = "https://api.digitalocean.com/v2/";

pub fn digital_ocean_token() -> String {
    std::env::var("DIGITAL_OCEAN_TOKEN").expect("env var DIGITAL_OCEAN_TOKEN is mandatory")
}

pub fn digital_ocean_spaces_access_id() -> String {
    std::env::var("DIGITAL_OCEAN_SPACES_ACCESS_ID")
        .expect("env var DIGITAL_OCEAN_SPACES_ACCESS_ID is mandatory")
}

pub fn digital_ocean_spaces_secret_key() -> String {
    std::env::var("DIGITAL_OCEAN_SPACES_SECRET_ID")
        .expect("env var DIGITAL_OCEAN_SPACES_SECRET_ID is mandatory")
}

pub fn container_registry_digital_ocean(context: &Context) -> DOCR {
    DOCR::new(
        context.clone(),
        "doea59qe62xaw3wj",
        "qovery-registry",
        digital_ocean_token().as_str(),
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
    let mut file = File::open("tests/assets/do-options.json").expect("file not found");
    let options_values = serde_json::from_reader(file).expect("JSON was not well-formatted");
    DOKS::<'a>::new(
        context.clone(),
        "my-first-doks-10",
        "do-kube-cluster-fra1-10",
        DO_KUBERNETES_VERSION,
        "fra1",
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
    ]
}

pub fn cloud_provider_digitalocean(context: &Context) -> DO {
    DO::new(
        context.clone(),
        "test",
        ORGANIZATION_ID,
        digital_ocean_token().as_str(),
        digital_ocean_spaces_access_id().as_str(),
        digital_ocean_spaces_secret_key().as_str(),
        "digital-ocean-test-cluster",
        TerraformStateCredentials {
            access_key_id: terraform_aws_access_key_id().to_string(),
            secret_access_key: terraform_aws_secret_access_key().to_string(),
            region: "eu-west-3".to_string(),
        },
    )
}

pub fn get_kube_cluster_name_from_uuid(uuid: &str) -> String {
    let mut headers = get_header_with_bearer(digital_ocean_token().as_str());
    let path = format!(
        "https://api.digitalocean.com/v2/kubernetes/clusters/{}",
        uuid
    );
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
                    Err(e) => panic!(e),
                }
            }
            _ => return String::from(""),
        },
        Err(e) => return String::from(""),
    }
}

use qovery_engine::cloud_provider::digitalocean::DO;

use crate::cloudflare::dns_provider_cloudflare;
use crate::utilities::build_platform_local_docker;
use qovery_engine::container_registry::docr::DOCR;
use qovery_engine::models::Context;
use qovery_engine::engine::Engine;

pub const DIGITAL_OCEAN_URL: &str = "https://api.digitalocean.com/v2/";

pub fn digital_ocean_token() -> String {
    std::env::var("DIGITAL_OCEAN_TOKEN").expect("env var DIGITAL_OCEAN_TOKEN is mandatory")
}

pub fn container_registry_digital_ocean(context: &Context) -> DOCR {
    DOCR::new(
        context.clone(),
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

pub fn cloud_provider_digitalocean(context: &Context) -> DO {
    DO::new(context.clone(), "test", digital_ocean_token().as_str())
}

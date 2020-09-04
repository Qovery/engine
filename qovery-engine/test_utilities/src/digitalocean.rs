use digitalocean::DigitalOcean;
use qovery_engine::container_registry::docr;
use qovery_engine::container_registry::docr::DOCR;

//TODO: should be environment var
pub const DIGITAL_OCEAN_TOKEN: &str =
    "34158dea3388309455954a9602be686de63b84ca6374db04588e818731ccf184";
pub const DIGITAL_OCEAN_URL: &str = "https://api.digitalocean.com/v2/";

pub fn container_registry_digital_ocean(context: &context) -> DOContainerRegistry {
    DOCR::new(context.clone(), "qovery-registry", DIGITAL_OCEAN_TOKEN)
}

pub fn docker_cr_do_engine(context: &Context) -> Engine {
    // use DigitalOcean Container Registry
    let container_registry = Box::new(container_registry_digital_ocean(context));
    // use LocalDocker
    let build_platform = Box::new(build_platform_local_docker(context));
    // use Digital Ocean
    let cloud_provider = Box::new(cloud_provider_digitalocean(context));
    Engine::new(
        context.clone(),
        build_platform,
        container_registry,
        cloud_provider,
    )
}

pub fn cloud_provider_digitalocean(context: &Context) -> DigitalOcean {
    DO::new(context.clone(), "test", DIGITAL_OCEAN_TOKEN)
}

use crate::helpers::utilities::FuncTestsSecrets;
use qovery_engine::dns_provider::cloudflare::Cloudflare;
use qovery_engine::models::Context;

pub fn dns_provider_cloudflare(context: &Context) -> Cloudflare {
    let secrets = FuncTestsSecrets::new();
    Cloudflare::new(
        context.clone(),
        "qoverytestdnsclo",
        "Qovery Test Cloudflare",
        secrets.CLOUDFLARE_DOMAIN.unwrap().as_str(),
        secrets.CLOUDFLARE_TOKEN.unwrap().as_str(), // Cloudflare name: Qovery test
        secrets.CLOUDFLARE_ID.unwrap().as_str(),
    )
}

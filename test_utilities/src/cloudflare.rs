use qovery_engine::dns_provider::cloudflare::Cloudflare;
use qovery_engine::models::Context;

pub const CLOUDFLARE_ID: String =
    std::env::var("CLOUDFLARE_ID").expect("env var CLOUDFLARE_ID is mandatory");

pub const CLOUDFLARE_TOKEN: String =
    std::env::var("CLOUDFLARE_TOKEN").expect("env var CLOUDFLARE_TOKEN is mandatory");

pub const CLOUDFLARE_DOMAIN: String =
    std::env::var("CLOUDFLARE_DOMAIN").expect("env var CLOUDFLARE_DOMAIN is mandatory");

pub fn dns_provider_cloudflare(context: &Context) -> Cloudflare {
    Cloudflare::new(
        context.clone(),
        "qoverytestdnsclo".to_string(),
        "Qovery Test Cloudflare".to_string(),
        CLOUDFLARE_DOMAIN.clone(),
        CLOUDFLARE_TOKEN.clone(), // Cloudflare name: Qovery test
        CLOUDFLARE_ID.clone(),
    )
}

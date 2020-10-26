use qovery_engine::dns_provider::cloudflare::Cloudflare;
use qovery_engine::models::Context;

pub fn cloudflare_id() -> String {
    std::env::var("CLOUDFLARE_ID").expect("env var CLOUDFLARE_ID is mandatory")
}

pub fn cloudflare_token() -> String {
    std::env::var("CLOUDFLARE_TOKEN").expect("env var CLOUDFLARE_TOKEN is mandatory")
}

pub fn cloudflare_domain() -> String {
    std::env::var("CLOUDFLARE_DOMAIN").expect("env var CLOUDFLARE_DOMAIN is mandatory")
}

pub fn dns_provider_cloudflare(context: &Context) -> Cloudflare {
    Cloudflare::new(
        context.clone(),
        "qoverytestdnsclo",
        "Qovery Test Cloudflare",
        cloudflare_domain().as_str(),
        cloudflare_token().as_str(), // Cloudflare name: Qovery test
        cloudflare_id().as_str(),
    )
}

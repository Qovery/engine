use qovery_engine::dns_provider::cloudflare::Cloudflare;
use qovery_engine::models::Context;

pub const CLOUDFLARE_ID: &str = "CHANGE ME";
pub const CLOUDFLARE_TOKEN: &str = "CHANGE ME";

pub fn dns_provider_cloudflare(context: &Context) -> Cloudflare {
    Cloudflare::new(
        context.clone(),
        "qoverytestdnsclo".to_string(),
        "Qovery Test Cloudflare".to_string(),
        "oom.sh".to_string(),
        CLOUDFLARE_TOKEN.to_string(), // Cloudflare name: Qovery test
        CLOUDFLARE_ID.to_string(),
    )
}

use qovery_engine::dns_provider::cloudflare::Cloudflare;
use qovery_engine::models::Context;

pub const CLOUDFLARE_ID: &str = "dns@qovery.com";
pub const CLOUDFLARE_TOKEN: &str = "9XhHmPprCG2OgLGhGEFEy7PxzOO_eydnxvtbRLn7";

pub fn dns_provider_cloudflare(context: &Context) -> Cloudflare {
    Cloudflare::new(
        context.clone(),
        "qoverytestdnsclo".to_string(),
        "Qovery Test Cloudflare".to_string(),
        "oom.sh".to_string(),
        "NJa2zDf603SU2Mveamf5INnFWsPZj3adPTxvjLlo".to_string(), // Cloudflare name: Qovery test
        "dns@qovery.com".to_string(),
    )
}

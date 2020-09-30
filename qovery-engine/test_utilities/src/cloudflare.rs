use qovery_engine::dns_provider::cloudflare::Cloudflare;
use qovery_engine::models::Context;

pub const CLOUDFLARE_ID: &str = "dns@qovery.com";
pub const CLOUDFLARE_TOKEN: &str = "9XhHmPprCG2OgLGhGEFEy7PxzOO_eydnxvtbRLn7";

pub fn dns_provider_cloudflare(context: &Context) -> Cloudflare {
    Cloudflare::new(
        context.clone(),
        "abc".to_string(),
        "default".to_string(),
        "qovery.io".to_string(),
        "9XhHmPprCG2OgLGhGEFEy7PxzOO_eydnxvtbRLn7".to_string(),
        "dns@qovery.com".to_string(),
    )
}

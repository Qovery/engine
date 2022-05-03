use crate::common::ClusterDomain;
use crate::utilities::FuncTestsSecrets;
use qovery_engine::dns_provider::cloudflare::Cloudflare;
use qovery_engine::dns_provider::DnsProvider;
use qovery_engine::io_models::{Context, Domain};

pub fn dns_provider_cloudflare(context: &Context, domain: &ClusterDomain) -> Box<dyn DnsProvider> {
    let secrets = FuncTestsSecrets::new();
    let domain = Domain::new(match domain {
        ClusterDomain::Custom(domain) => domain.to_string(),
        ClusterDomain::Default { cluster_id } => format!(
            "{}.{}",
            cluster_id,
            secrets.CLOUDFLARE_DOMAIN.expect("CLOUDFLARE_DOMAIN is not set")
        ),
    });
    Box::new(Cloudflare::new(
        context.clone(),
        "qoverytestdnsclo",
        "Qovery Test Cloudflare",
        domain,
        secrets.CLOUDFLARE_TOKEN.expect("CLOUDFLARE_TOKEN is not set").as_str(), // Cloudflare name: Qovery test
        secrets.CLOUDFLARE_ID.expect("CLOUDFLARE_ID is not set").as_str(),
    ))
}

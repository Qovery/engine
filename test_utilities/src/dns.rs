use crate::common::ClusterDomain;
use crate::utilities::FuncTestsSecrets;
use qovery_engine::dns_provider::cloudflare::Cloudflare;
use qovery_engine::dns_provider::qoverydns::QoveryDns;
use qovery_engine::dns_provider::DnsProvider;
use qovery_engine::io_models::{Context, Domain};
use url::Url;

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

pub fn dns_provider_qoverydns(context: &Context, domain: &ClusterDomain) -> Box<dyn DnsProvider> {
    let secrets = FuncTestsSecrets::new();
    let domain = Domain::new(match domain {
        ClusterDomain::Custom(domain) => domain.to_string(),
        ClusterDomain::Default { cluster_id } => format!(
            "{}.{}",
            cluster_id,
            secrets.CLOUDFLARE_DOMAIN.expect("QOVERYDNS_DOMAIN is not set")
        ),
    });
    Box::new(QoveryDns::new(
        context.clone(),
        "qoverytestdnsqdns",
        Url::parse(
            secrets
                .QOVERY_DNS_API_URL
                .expect("QOVERY_DNS_API_URL is not set")
                .as_str(),
        )
        .expect("QOVERY_DNS_API_URL is not a valid URL"),
        secrets
            .QOVERY_DNS_API_KEY
            .expect("QOVERY_DNS_API_KEY is not set")
            .as_str(),
        "Qovery Test QoveryDNS",
        domain,
    ))
}

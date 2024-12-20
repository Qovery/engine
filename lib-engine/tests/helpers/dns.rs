use crate::helpers::common::ClusterDomain;
use crate::helpers::utilities::FuncTestsSecrets;
use qovery_engine::environment::models::domain::Domain;
use qovery_engine::infrastructure::models::dns_provider::cloudflare::Cloudflare;
use qovery_engine::infrastructure::models::dns_provider::qoverydns::QoveryDns;
use qovery_engine::infrastructure::models::dns_provider::DnsProvider;
use qovery_engine::io_models::context::Context;
use url::Url;
use uuid::Uuid;

pub fn dns_provider_cloudflare(context: &Context, domain: &ClusterDomain) -> Box<dyn DnsProvider> {
    let secrets = FuncTestsSecrets::new();
    let domain = Domain::new(match domain {
        ClusterDomain::Custom { domain } => domain.to_string(),
        ClusterDomain::Default { cluster_id } => format!(
            "{}.{}",
            cluster_id,
            secrets.CLOUDFLARE_DOMAIN.expect("CLOUDFLARE_DOMAIN is not set")
        ),
        ClusterDomain::QoveryOwnedDomain { cluster_id, domain } => format!("{cluster_id}.{domain}",),
    });
    Box::new(Cloudflare::new(
        context.clone(),
        Uuid::new_v4(),
        "Qovery Test Cloudflare",
        domain,
        secrets.CLOUDFLARE_TOKEN.expect("CLOUDFLARE_TOKEN is not set").as_str(), // Cloudflare name: Qovery test
        secrets.CLOUDFLARE_ID.expect("CLOUDFLARE_ID is not set").as_str(),
        false,
    ))
}

pub fn dns_provider_qoverydns(context: &Context, cluster_domain: &ClusterDomain) -> Box<dyn DnsProvider> {
    let secrets = FuncTestsSecrets::new();
    let domain = Domain::new(match cluster_domain {
        ClusterDomain::Custom { domain } => domain.to_string(),
        ClusterDomain::Default { cluster_id } => format!(
            "{}.{}",
            cluster_id,
            secrets.CLOUDFLARE_DOMAIN.expect("CLOUDFLARE_DOMAIN is not set")
        ),
        ClusterDomain::QoveryOwnedDomain { cluster_id, domain } => format!("{cluster_id}.{domain}",),
    });
    Box::new(QoveryDns::new(
        context.clone(),
        Uuid::new_v4(),
        Url::parse(
            secrets
                .QOVERY_DNS_API_URL
                .expect("QOVERY_DNS_API_URL is not set")
                .as_str(),
        )
        .expect("QOVERY_DNS_API_URL is not a valid URL"),
        secrets
            .QOVERY_CLUSTER_JWT_TOKEN
            .expect("QOVERY_CLUSTER_JWT_TOKEN is not set")
            .as_str(),
        "Qovery Test QoveryDNS",
        domain,
    ))
}

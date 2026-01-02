use std::net::Ipv4Addr;

use crate::events::{EventDetails, InfrastructureStep, Stage, Transmitter};
use crate::infrastructure::models::dns_provider::cloudflare::CloudflareDnsConfig;
use crate::infrastructure::models::dns_provider::errors::DnsProviderError;
use crate::infrastructure::models::dns_provider::qoverydns::QoveryDnsConfig;
use crate::infrastructure::models::dns_provider::route53::Route53DnsConfig;
use tera::Context as TeraContext;
use uuid::Uuid;

use crate::environment::models::domain::Domain;
use crate::io_models::QoveryIdentifier;
use crate::io_models::context::Context;

pub mod cloudflare;
pub mod errors;
pub mod io;
pub mod qoverydns;
pub mod route53;

#[derive(Clone, Debug)]
pub enum Kind {
    Cloudflare,
    QoveryDns,
    Route53,
}

#[derive(Clone, Debug)]
pub enum DnsProviderConfiguration {
    Cloudflare(CloudflareDnsConfig),
    QoveryDns(QoveryDnsConfig),
    Route53(Route53DnsConfig),
}

impl DnsProviderConfiguration {
    pub fn get_cert_manager_config_name(&self) -> String {
        match self {
            DnsProviderConfiguration::Cloudflare(_) => "cloudflare",
            DnsProviderConfiguration::QoveryDns(_) => "pdns",
            DnsProviderConfiguration::Route53(_) => "route53",
        }
        .to_string()
    }

    pub fn get_external_dns_provider_name(&self) -> String {
        match self {
            DnsProviderConfiguration::Cloudflare(_) => "cloudflare",
            DnsProviderConfiguration::QoveryDns(_) => "pdns",
            DnsProviderConfiguration::Route53(_) => "aws",
        }
        .to_string()
    }
}

pub trait DnsProvider: Send + Sync {
    fn context(&self) -> &Context;
    fn provider_name(&self) -> &str;
    fn kind(&self) -> Kind;
    fn long_id(&self) -> &Uuid;
    fn name(&self) -> &str;
    fn insert_into_teracontext<'a>(&self, context: &'a mut TeraContext) -> &'a mut TeraContext;
    fn provider_configuration(&self) -> DnsProviderConfiguration;
    fn domain(&self) -> &Domain;
    fn resolvers(&self) -> Vec<Ipv4Addr>;
    fn is_valid(&self) -> Result<(), DnsProviderError>;
    fn event_details(&self) -> EventDetails {
        EventDetails::new(
            None,
            QoveryIdentifier::new(*self.context().organization_long_id()),
            QoveryIdentifier::new(*self.context().cluster_long_id()),
            self.context().execution_id().to_string(),
            Stage::Infrastructure(InfrastructureStep::ValidateSystemRequirements),
            Transmitter::DnsProvider(*self.long_id(), self.provider_name().to_string()),
        )
    }
}

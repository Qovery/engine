use std::net::Ipv4Addr;

use crate::dns_provider::cloudflare::CloudflareDnsConfig;
use crate::dns_provider::errors::DnsProviderError;
use crate::dns_provider::qoverydns::QoveryDnsConfig;
use tera::Context as TeraContext;

use crate::io_models::context::Context;
use crate::io_models::domain::Domain;

pub mod cloudflare;
pub mod errors;
pub mod io;
pub mod qoverydns;

#[derive(Clone, Debug)]
pub enum Kind {
    Cloudflare,
    QoveryDns,
}

pub enum DnsProviderConfiguration {
    Cloudflare(CloudflareDnsConfig),
    QoveryDns(QoveryDnsConfig),
}

impl DnsProviderConfiguration {
    pub fn get_cert_manager_config_name(&self) -> String {
        match self {
            DnsProviderConfiguration::Cloudflare(_) => "cloudflare",
            DnsProviderConfiguration::QoveryDns(_) => "pdns",
        }
        .to_string()
    }
}

pub trait DnsProvider {
    fn context(&self) -> &Context;
    fn provider_name(&self) -> &str;
    fn kind(&self) -> Kind;
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn name_with_id(&self) -> String {
        format!("{} ({})", self.name(), self.id())
    }
    fn insert_into_teracontext<'a>(&self, context: &'a mut TeraContext) -> &'a mut TeraContext;
    fn provider_configuration(&self) -> DnsProviderConfiguration;
    fn domain(&self) -> &Domain;
    fn resolvers(&self) -> Vec<Ipv4Addr>;
    fn is_valid(&self) -> Result<(), DnsProviderError>;
}

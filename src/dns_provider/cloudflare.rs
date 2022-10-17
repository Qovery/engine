use std::net::Ipv4Addr;
use tera::Context as TeraContext;
use uuid::Uuid;

use crate::dns_provider::errors::DnsProviderError;
use crate::dns_provider::{DnsProvider, DnsProviderConfiguration, Kind};
use crate::io_models::context::Context;
use crate::io_models::domain::Domain;

#[derive(Clone, Debug)]
pub struct CloudflareDnsConfig {
    pub cloudflare_email: String,
    pub cloudflare_api_token: String,
}

pub struct Cloudflare {
    context: Context,
    long_id: Uuid,
    name: String,
    domain: Domain,
    cloudflare_api_token: String,
    cloudflare_email: String,
}

impl Cloudflare {
    pub fn new(
        context: Context,
        long_id: Uuid,
        name: &str,
        domain: Domain,
        cloudflare_api_token: &str,
        cloudflare_email: &str,
    ) -> Self {
        Cloudflare {
            context,
            long_id,
            name: name.to_string(),
            domain,
            cloudflare_api_token: cloudflare_api_token.to_string(),
            cloudflare_email: cloudflare_email.to_string(),
        }
    }
}

impl DnsProvider for Cloudflare {
    fn context(&self) -> &Context {
        &self.context
    }

    fn provider_name(&self) -> &str {
        "cloudflare"
    }

    fn kind(&self) -> Kind {
        Kind::Cloudflare
    }

    fn long_id(&self) -> &Uuid {
        &self.long_id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn insert_into_teracontext<'a>(&self, context: &'a mut TeraContext) -> &'a mut TeraContext {
        context.insert("external_dns_provider", &self.provider_name());
        context.insert("cloudflare_email", &self.cloudflare_email);
        context.insert("cloudflare_api_token", &self.cloudflare_api_token);
        context
    }

    fn provider_configuration(&self) -> DnsProviderConfiguration {
        DnsProviderConfiguration::Cloudflare(CloudflareDnsConfig {
            cloudflare_email: self.cloudflare_email.clone(),
            cloudflare_api_token: self.cloudflare_api_token.clone(),
        })
    }

    fn domain(&self) -> &Domain {
        &self.domain
    }

    fn resolvers(&self) -> Vec<Ipv4Addr> {
        vec![Ipv4Addr::new(1, 1, 1, 1), Ipv4Addr::new(1, 0, 0, 1)]
    }

    fn is_valid(&self) -> Result<(), DnsProviderError> {
        if self.cloudflare_api_token.is_empty() || self.cloudflare_email.is_empty() {
            Err(DnsProviderError::InvalidCredentials)
        } else {
            Ok(())
        }
    }
}

use std::net::Ipv4Addr;

use crate::dns_provider::{DnsProvider, DnsProviderError, Kind};
use crate::models::Context;

pub struct Cloudflare {
    context: Context,
    id: String,
    name: String,
    domain: String,
    cloudflare_api_token: String,
    cloudflare_email: String,
}

impl Cloudflare {
    pub fn new(
        context: Context,
        id: &str,
        name: &str,
        domain: &str,
        cloudflare_api_token: &str,
        cloudflare_email: &str,
    ) -> Self {
        Cloudflare {
            context,
            id: id.to_string(),
            name: name.to_string(),
            domain: domain.to_string(),
            cloudflare_api_token: cloudflare_api_token.to_string(),
            cloudflare_email: cloudflare_email.to_string(),
        }
    }
}

impl DnsProvider for Cloudflare {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::CLOUDFLARE
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn account(&self) -> &str {
        &self.cloudflare_email
    }

    fn token(&self) -> &str {
        &self.cloudflare_api_token
    }

    fn domain(&self) -> &str {
        self.domain.as_str()
    }

    fn resolvers(&self) -> Vec<Ipv4Addr> {
        vec![Ipv4Addr::new(1, 1, 1, 1), Ipv4Addr::new(1, 0, 0, 1)]
    }

    fn is_valid(&self) -> Result<(), DnsProviderError> {
        if self.cloudflare_api_token.is_empty() || self.cloudflare_email.is_empty() {
            Err(DnsProviderError::Credentials)
        } else {
            Ok(())
        }
    }
}

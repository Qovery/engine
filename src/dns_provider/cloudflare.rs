use std::net::Ipv4Addr;

use crate::dns_provider::{DnsProvider, Kind};
use crate::errors::EngineError;
use crate::events::{ToTransmitter, Transmitter};
use crate::models::{Context, Domain};

pub struct Cloudflare {
    context: Context,
    id: String,
    name: String,
    domain: Domain,
    cloudflare_api_token: String,
    cloudflare_email: String,
}

impl Cloudflare {
    pub fn new(
        context: Context,
        id: &str,
        name: &str,
        domain: Domain,
        cloudflare_api_token: &str,
        cloudflare_email: &str,
    ) -> Self {
        Cloudflare {
            context,
            id: id.to_string(),
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

    fn domain(&self) -> &Domain {
        &self.domain
    }

    fn resolvers(&self) -> Vec<Ipv4Addr> {
        vec![Ipv4Addr::new(1, 1, 1, 1), Ipv4Addr::new(1, 0, 0, 1)]
    }

    fn is_valid(&self) -> Result<(), EngineError> {
        if self.cloudflare_api_token.is_empty() || self.cloudflare_email.is_empty() {
            Err(EngineError::new_client_invalid_cloud_provider_credentials(self.get_event_details()))
        } else {
            Ok(())
        }
    }
}

impl ToTransmitter for Cloudflare {
    fn to_transmitter(&self) -> Transmitter {
        Transmitter::DnsProvider(self.id().to_string(), self.name().to_string())
    }
}

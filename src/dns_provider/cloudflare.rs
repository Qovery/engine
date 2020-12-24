use std::net::Ipv4Addr;

use crate::dns_provider::{DnsProvider, Kind};
use crate::error::{EngineError, EngineErrorCause};
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

    fn domain(&self) -> &str {
        self.domain.as_str()
    }

    fn resolvers(&self) -> Vec<Ipv4Addr> {
        vec![Ipv4Addr::new(1, 1, 1, 1), Ipv4Addr::new(1, 0, 0, 1)]
    }

    fn is_valid(&self) -> Result<(), EngineError> {
        if self.cloudflare_api_token.is_empty() || self.cloudflare_email.is_empty() {
            Err(self.engine_error(
                EngineErrorCause::User(
                    "Your Cloudflare account seems to be no longer valid (bad Credentials). \
                    Please contact your Organization administrator to fix or change the Credentials.",
                ),
                format!("bad Cloudflare credentials for {}", self.name_with_id()),
            ))
        } else {
            Ok(())
        }
    }
}

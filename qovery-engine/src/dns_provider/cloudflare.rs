use crate::dns_provider::{DnsProvider, DnsProviderError, Kind};
use crate::models::Context;

pub struct Cloudflare {
    context: Context,
    id: String,
    name: String,
    cloudflare_api_token: String,
    cloudflare_email: String,
}

impl Cloudflare {
    pub fn new(context: Context, id: String, name: String, cloudflare_api_token: String, cloudflare_email: String) -> Self {
        Cloudflare {
            context,
            id,
            name,
            cloudflare_api_token,
            cloudflare_email
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

    fn password(&self) -> &str {
        &self.cloudflare_api_token
    }

    fn is_valid(&self) -> Result<(), DnsProviderError> {
        if self.cloudflare_api_token.is_empty() || self.cloudflare_email.is_empty() {
            Err(DnsProviderError::Credentials)
        } else {
            Ok(())
        }
    }
}

use std::net::Ipv4Addr;
use tera::Context as TeraContext;
use url::Url;
use uuid::Uuid;

use crate::environment::models::domain::Domain;
use crate::infrastructure::models::dns_provider::errors::DnsProviderError;
use crate::infrastructure::models::dns_provider::Kind;
use crate::infrastructure::models::dns_provider::{DnsProvider, DnsProviderConfiguration};
use crate::io_models::context::Context;

#[derive(Clone, Debug)]
pub struct QoveryDnsConfig {
    pub api_url: Url,
    pub api_key: String,

    // TODO(benjaminch): fields bellow are far from optimal, but since PDNS requires weird URL and port, better to do it once here
    pub api_url_scheme_and_domain: String,
    pub api_url_port: String,
}

pub struct QoveryDns {
    context: Context,
    long_id: Uuid,
    name: String,
    domain: Domain,
    dns_config: QoveryDnsConfig,
}

impl QoveryDns {
    pub fn new(context: Context, long_id: Uuid, api_url: Url, api_key: &str, name: &str, domain: Domain) -> Self {
        let mut api_port = "".to_string();
        let mut api_url_scheme_and_domain = "".to_string();

        if let Some(domain) = api_url.domain() {
            api_url_scheme_and_domain = format!("{}://{}", api_url.scheme(), domain);
        }
        // Note: .port() seems to be broken, using .port_or_known_default() instead
        // let url = Url::parse("https://ddns.qovery.com:443").unwrap();
        // println!("{:?}", url.port_or_known_default());
        // println!("{:?}", url.port());
        // => print Some(443)
        // => print None
        if let Some(p) = api_url.port_or_known_default() {
            api_port = p.to_string();
        }

        QoveryDns {
            context,
            long_id,
            name: name.to_string(),
            domain,
            dns_config: QoveryDnsConfig {
                api_url,
                api_url_scheme_and_domain,
                api_url_port: api_port,
                api_key: api_key.to_string(),
            },
        }
    }
}

impl DnsProvider for QoveryDns {
    fn context(&self) -> &Context {
        &self.context
    }

    fn provider_name(&self) -> &str {
        "pdns"
    }

    fn kind(&self) -> Kind {
        Kind::QoveryDns
    }

    fn long_id(&self) -> &Uuid {
        &self.long_id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn insert_into_teracontext<'a>(&self, context: &'a mut TeraContext) -> &'a mut TeraContext {
        // PDNS requires URL and port to be sent as different fields
        context.insert("external_dns_provider", &self.provider_name());
        context.insert("qoverydns_api_url", &self.dns_config.api_url_scheme_and_domain);
        context.insert("qoverydns_api_port", &self.dns_config.api_url_port);
        context.insert("qoverydns_api_key", &self.dns_config.api_key);
        context
    }

    fn provider_configuration(&self) -> DnsProviderConfiguration {
        DnsProviderConfiguration::QoveryDns(self.dns_config.clone())
    }

    fn domain(&self) -> &Domain {
        &self.domain
    }

    fn resolvers(&self) -> Vec<Ipv4Addr> {
        vec![Ipv4Addr::new(8, 8, 8, 8), Ipv4Addr::new(8, 8, 4, 4)]
    }

    fn is_valid(&self) -> Result<(), DnsProviderError> {
        if self.dns_config.api_key.is_empty() {
            return Err(DnsProviderError::InvalidCredentials);
        }
        if self.dns_config.api_url.domain().is_none() {
            return Err(DnsProviderError::InvalidApiUrl);
        }

        Ok(())
    }
}

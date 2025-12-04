use std::net::Ipv4Addr;
use tera::Context as TeraContext;
use uuid::Uuid;

use crate::environment::models::domain::Domain;
use crate::infrastructure::models::dns_provider::errors::DnsProviderError;
use crate::infrastructure::models::dns_provider::{DnsProvider, DnsProviderConfiguration, Kind};
use crate::io_models::context::Context;

#[derive(Clone, Debug)]
pub struct Route53DnsConfig {
    pub aws_access_key_id: String,
    pub aws_secret_access_key: String,
    pub aws_region: String,
    pub hosted_zone_id: Option<String>,
}

pub struct Route53 {
    context: Context,
    long_id: Uuid,
    name: String,
    domain: Domain,
    aws_access_key_id: String,
    aws_secret_access_key: String,
    aws_region: String,
    hosted_zone_id: Option<String>,
}

impl Route53 {
    pub fn new(
        context: Context,
        long_id: Uuid,
        name: &str,
        domain: Domain,
        aws_access_key_id: &str,
        aws_secret_access_key: &str,
        aws_region: &str,
        hosted_zone_id: Option<String>,
    ) -> Self {
        Route53 {
            context,
            long_id,
            name: name.to_string(),
            domain,
            aws_access_key_id: aws_access_key_id.to_string(),
            aws_secret_access_key: aws_secret_access_key.to_string(),
            aws_region: aws_region.to_string(),
            hosted_zone_id,
        }
    }
}

impl DnsProvider for Route53 {
    fn context(&self) -> &Context {
        &self.context
    }

    fn provider_name(&self) -> &str {
        "route53"
    }

    fn kind(&self) -> Kind {
        Kind::Route53
    }

    fn long_id(&self) -> &Uuid {
        &self.long_id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn insert_into_teracontext<'a>(&self, context: &'a mut TeraContext) -> &'a mut TeraContext {
        context.insert("external_dns_provider", &self.provider_name());
        context.insert("aws_access_key_id", &self.aws_access_key_id);
        context.insert("aws_secret_access_key", &self.aws_secret_access_key);
        context.insert("aws_region", &self.aws_region);
        if let Some(hosted_zone_id) = &self.hosted_zone_id {
            context.insert("hosted_zone_id", hosted_zone_id);
        }
        context
    }

    fn provider_configuration(&self) -> DnsProviderConfiguration {
        DnsProviderConfiguration::Route53(Route53DnsConfig {
            aws_access_key_id: self.aws_access_key_id.clone(),
            aws_secret_access_key: self.aws_secret_access_key.clone(),
            aws_region: self.aws_region.clone(),
            hosted_zone_id: self.hosted_zone_id.clone(),
        })
    }

    fn domain(&self) -> &Domain {
        &self.domain
    }

    fn resolvers(&self) -> Vec<Ipv4Addr> {
        // AWS Route 53 public resolvers (using Google DNS as fallback)
        vec![Ipv4Addr::new(8, 8, 8, 8), Ipv4Addr::new(8, 8, 4, 4)]
    }

    fn is_valid(&self) -> Result<(), DnsProviderError> {
        if self.aws_access_key_id.is_empty() || self.aws_secret_access_key.is_empty() || self.aws_region.is_empty() {
            Err(DnsProviderError::InvalidCredentials)
        } else {
            Ok(())
        }
    }
}

use std::net::Ipv4Addr;

use crate::dns_provider::errors::DnsProviderError;
use serde::{Deserialize, Serialize};

use crate::io_models::{Context, Domain};

pub mod cloudflare;
pub mod errors;

pub trait DnsProvider {
    fn context(&self) -> &Context;
    fn provider_name(&self) -> &str;
    fn kind(&self) -> Kind;
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn name_with_id(&self) -> String {
        format!("{} ({})", self.name(), self.id())
    }
    fn account(&self) -> &str;
    fn token(&self) -> &str;
    fn domain(&self) -> &Domain;
    fn resolvers(&self) -> Vec<Ipv4Addr>;
    fn is_valid(&self) -> Result<(), DnsProviderError>;
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Kind {
    Cloudflare,
}

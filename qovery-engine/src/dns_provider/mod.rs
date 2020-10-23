use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

use crate::models::Context;

pub mod cloudflare;

#[derive(Serialize, Deserialize, Clone)]
pub enum Kind {
    CLOUDFLARE,
}

pub trait DnsProvider {
    fn context(&self) -> &Context;
    fn kind(&self) -> Kind;
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn account(&self) -> &str;
    fn token(&self) -> &str;
    fn domain(&self) -> &str;
    fn resolvers(&self) -> Vec<Ipv4Addr>;
    fn is_valid(&self) -> Result<(), DnsProviderError>;
}

#[derive(Debug, Eq, PartialEq)]
pub enum DnsProviderError {
    Credentials,
    Unknown,
}

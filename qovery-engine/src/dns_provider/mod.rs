use crate::models::Context;
use serde::{Deserialize, Serialize};

pub mod cloudflare;

#[derive(Serialize, Deserialize, Clone)]
pub enum Kind {
    CLOUDFLARE
}

pub trait DnsProvider {
    fn context(&self) -> &Context;
    fn kind(&self) -> Kind;
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn account(&self) -> &str;
    fn password(&self) -> &str;
    fn is_valid(&self) -> Result<(), DnsProviderError>;
}

#[derive(Debug, Eq, PartialEq)]
pub enum DnsProviderError {
    Credentials,
    Unknown,
}

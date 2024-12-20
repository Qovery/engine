use crate::infrastructure::models::dns_provider;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Kind {
    Cloudflare,
    QoveryDns,
}

impl From<dns_provider::Kind> for Kind {
    fn from(kind: dns_provider::Kind) -> Self {
        match kind {
            dns_provider::Kind::Cloudflare => Kind::Cloudflare,
            dns_provider::Kind::QoveryDns => Kind::QoveryDns,
        }
    }
}

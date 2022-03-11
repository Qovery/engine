use std::net::Ipv4Addr;

use crate::errors::EngineError;
use crate::events::{EnvironmentStep, EventDetails, Stage, ToTransmitter};
use serde::{Deserialize, Serialize};

use crate::models::{Context, Domain, QoveryIdentifier};

pub mod cloudflare;

pub trait DnsProvider: ToTransmitter {
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
    fn is_valid(&self) -> Result<(), EngineError>;
    fn get_event_details(&self) -> EventDetails {
        let context = self.context();
        EventDetails::new(
            None,
            QoveryIdentifier::from(context.organization_id().to_string()),
            QoveryIdentifier::from(context.cluster_id().to_string()),
            QoveryIdentifier::from(context.execution_id().to_string()),
            None,
            Stage::Environment(EnvironmentStep::Deploy),
            self.to_transmitter(),
        )
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Kind {
    Cloudflare,
}

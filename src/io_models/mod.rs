use crate::cloud_provider::service;
use crate::utilities::to_short_id;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use uuid::Uuid;

pub mod application;
pub mod container;
pub mod context;
pub mod database;
pub mod domain;
pub mod environment;
pub mod progress_listener;
pub mod router;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QoveryIdentifier {
    long_id: Uuid,
    short: String,
}

impl QoveryIdentifier {
    pub fn new(long_id: Uuid) -> Self {
        QoveryIdentifier {
            long_id,
            short: to_short_id(&long_id),
        }
    }

    pub fn new_random() -> Self {
        Self::new(Uuid::new_v4())
    }

    pub fn short(&self) -> &str {
        &self.short
    }
}

impl Default for QoveryIdentifier {
    fn default() -> Self {
        QoveryIdentifier::new_random()
    }
}

impl Display for QoveryIdentifier {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.long_id.to_string().as_str())
    }
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Action {
    Create,
    Pause,
    Delete,
    Nothing,
}

impl Action {
    pub fn to_service_action(&self) -> service::Action {
        match self {
            Action::Create => service::Action::Create,
            Action::Pause => service::Action::Pause,
            Action::Delete => service::Action::Delete,
            Action::Nothing => service::Action::Nothing,
        }
    }
}

use serde::{Deserialize, Serialize};

use crate::build_platform::Image;
use crate::errors::EngineError;
use crate::events::{EnvironmentStep, EventDetails, Stage, ToTransmitter};
use crate::logger::Logger;
use crate::models::{Context, Listen, QoveryIdentifier};

pub mod docker;
pub mod docker_hub;
pub mod docr;
pub mod ecr;
pub mod scaleway_container_registry;

pub trait ContainerRegistry: Listen + ToTransmitter {
    fn context(&self) -> &Context;
    fn kind(&self) -> Kind;
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn name_with_id(&self) -> String {
        format!("{} ({})", self.name(), self.id())
    }
    fn is_valid(&self) -> Result<(), EngineError>;
    fn on_create(&self) -> Result<(), EngineError>;
    fn on_create_error(&self) -> Result<(), EngineError>;
    fn on_delete(&self) -> Result<(), EngineError>;
    fn on_delete_error(&self) -> Result<(), EngineError>;
    fn does_image_exists(&self, image: &Image) -> bool;
    fn pull(&self, image: &Image) -> Result<PullResult, EngineError>;
    fn push(&self, image: &Image, force_push: bool) -> Result<PushResult, EngineError>;
    fn push_error(&self, image: &Image) -> Result<PushResult, EngineError>;
    fn logger(&self) -> &dyn Logger;
    fn get_event_details(&self) -> EventDetails {
        let context = self.context();
        EventDetails::new(
            None,
            QoveryIdentifier::from(context.organization_id().to_string()),
            QoveryIdentifier::from(context.cluster_id().to_string()),
            QoveryIdentifier::from(context.execution_id().to_string()),
            None,
            Stage::Environment(EnvironmentStep::Build),
            self.to_transmitter(),
        )
    }
}

pub struct PushResult {
    pub image: Image,
}

pub enum PullResult {
    Some(Image),
    None,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Kind {
    DockerHub,
    Ecr,
    Docr,
    ScalewayCr,
}

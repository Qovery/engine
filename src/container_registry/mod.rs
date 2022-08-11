use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::build_platform::Image;
use crate::container_registry::errors::ContainerRegistryError;
use crate::errors::EngineError;
use crate::events::{EventDetails, Stage, Transmitter};
use crate::io_models::context::Context;
use crate::io_models::QoveryIdentifier;

pub mod docr;
pub mod ecr;
pub mod errors;
pub mod scaleway_container_registry;

pub trait ContainerRegistry {
    fn context(&self) -> &Context;
    fn kind(&self) -> Kind;
    fn id(&self) -> &str;
    fn long_id(&self) -> &Uuid;
    fn name(&self) -> &str;
    fn name_with_id(&self) -> String {
        format!("{} ({})", self.name(), self.id())
    }

    // Get info for this registry, url endpoint with login/password, image name convention, ...
    fn registry_info(&self) -> &ContainerRegistryInfo;

    // Some provider require specific action in order to allow container registry
    // For now it is only digital ocean, that require 2 steps to have registries
    fn create_registry(&self) -> Result<(), ContainerRegistryError>;

    // Call to create a specific repository in the registry
    // i.e: docker.io/erebe or docker.io/qovery
    // All providers requires action for that
    // The convention for us is that we create one per application
    fn create_repository(&self, repository_name: &str) -> Result<(), ContainerRegistryError>;
    fn delete_repository(&self, repository_name: &str) -> Result<(), ContainerRegistryError>;

    fn delete_image(&self, image_name: &Image) -> Result<(), ContainerRegistryError>;

    // Check on the registry if a specific image already exist
    fn does_image_exists(&self, image: &Image) -> bool;

    fn get_event_details(&self, stage: Stage) -> EventDetails {
        let context = self.context();
        let ev = EventDetails::new(
            None,
            QoveryIdentifier::from(context.organization_id().to_string()),
            QoveryIdentifier::from(context.cluster_id().to_string()),
            QoveryIdentifier::from(context.execution_id().to_string()),
            None,
            stage,
            Transmitter::ContainerRegistry(self.id().to_string(), self.name().to_string()),
        );

        ev
    }
}

pub fn to_engine_error(event_details: EventDetails, err: ContainerRegistryError) -> EngineError {
    EngineError::new_container_registry_error(event_details, err)
}

pub struct ContainerRegistryInfo {
    pub endpoint: Url, // Contains username and password if necessary
    pub registry_name: String,
    pub registry_docker_json_config: Option<String>,
    // give it the name of your image, and it returns the full name with prefix if needed
    // i.e: for DigitalOcean => registry_name/image_name
    // i.e: fo scaleway => image_name/image_name
    // i.e: for AWS => image_name
    pub get_image_name: Box<dyn Fn(&str) -> String>,

    // Give it the name of your image, and it return the name of the repository that will be used
    pub get_repository_name: Box<dyn Fn(&str) -> String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Kind {
    Ecr,
    Docr,
    ScalewayCr,
}

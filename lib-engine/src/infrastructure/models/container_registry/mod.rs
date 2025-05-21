use enum_dispatch::enum_dispatch;
use std::collections::HashMap;
use std::time::Duration;
use url::Url;
use uuid::Uuid;

use crate::errors::EngineError;
use crate::events::{EventDetails, Stage, Transmitter};
use crate::infrastructure::models::build_platform::Image;
use crate::infrastructure::models::container_registry::errors::ContainerRegistryError;
use crate::io_models::QoveryIdentifier;
use crate::io_models::context::Context;

pub mod azure_container_registry;
pub mod ecr;
pub mod errors;
pub mod generic_cr;
pub mod github_cr;
pub mod google_artifact_registry;
pub mod scaleway_container_registry;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repository {
    pub registry_id: String,
    pub name: String,
    pub uri: Option<String>,
    pub ttl: Option<Duration>,
    pub labels: Option<HashMap<String, String>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DockerImage {
    pub repository_id: String,
    pub name: String,
    pub tag: String,
}

pub struct RegistryTags {
    pub cluster_id: Option<String>,
    pub environment_id: Option<String>,
    pub project_id: Option<String>,
    pub resource_ttl: Option<Duration>,
}

#[enum_dispatch]
pub trait InteractWithRegistry: Send + Sync {
    fn context(&self) -> &Context;
    fn kind(&self) -> Kind;
    fn long_id(&self) -> &Uuid;
    fn name(&self) -> &str;

    // Get info for this registry, url endpoint with login/password, image name convention, ...
    fn registry_info(&self) -> &ContainerRegistryInfo;

    fn get_registry_endpoint(&self, registry_endpoint_prefix: Option<&str>) -> Url;

    // Call to create a specific repository in the registry
    // i.e: docker.io/erebe or docker.io/qovery
    // All providers requires action for that
    // The convention for us is that we create one per cluster/git_repo_url
    // DEPRECATED: The convention for us is that we create one per application
    fn create_repository(
        &self,
        registry_name: Option<&str>,
        repository_name: &str,
        image_retention_time_in_seconds: u32,
        registry_tags: RegistryTags,
    ) -> Result<(Repository, RepositoryInfo), ContainerRegistryError>;

    fn get_repository(&self, repository_name: &str) -> Result<Repository, ContainerRegistryError>;

    fn delete_repository(&self, repository_name: &str) -> Result<(), ContainerRegistryError>;

    fn delete_image(&self, image_name: &Image) -> Result<(), ContainerRegistryError>;

    // Check on the registry if a specific image already exists
    fn image_exists(&self, image: &Image) -> bool;

    fn get_event_details(&self, stage: Stage) -> EventDetails {
        let context = self.context();
        let ev = EventDetails::new(
            None,
            QoveryIdentifier::new(*context.organization_long_id()),
            QoveryIdentifier::new(*context.cluster_long_id()),
            context.execution_id().to_string(),
            stage,
            Transmitter::ContainerRegistry(*self.long_id(), self.name().to_string()),
        );

        ev
    }
}

#[enum_dispatch(InteractWithRegistry)]
pub enum ContainerRegistry {
    Ecr(ecr::ECR),
    AzureContainerRegistry(azure_container_registry::AzureContainerRegistry),
    ScalewayCr(scaleway_container_registry::ScalewayCR),
    GcpArtifactRegistry(google_artifact_registry::GoogleArtifactRegistry),
    GenericCr(generic_cr::GenericCr),
    GithubCr(github_cr::GithubCr),
}

impl ContainerRegistry {
    pub fn as_azure_container_registry(
        &self,
    ) -> Result<&azure_container_registry::AzureContainerRegistry, ContainerRegistryError> {
        match self {
            ContainerRegistry::AzureContainerRegistry(azure_cr) => Ok(azure_cr),
            _ => Err(ContainerRegistryError::CannotConvertClient {
                raw_error_message: "Cannot convert client to AzureContainerRegistry".to_string(),
            }),
        }
    }
}

pub fn to_engine_error(event_details: EventDetails, err: ContainerRegistryError) -> EngineError {
    EngineError::new_container_registry_error(event_details, err)
}

pub struct ImageBuildContext {
    pub cluster_id: QoveryIdentifier,
    pub git_repo_url_sanitized: String,
}

type RegistryName = String;
type RepositoryName = String;
type ImageName = String;

pub struct DockerRegistryInfo {
    pub registry_name: Option<RegistryName>,
    pub repository_name: Option<RepositoryName>,
    pub image_name: Option<ImageName>,
}

pub type RegistryEndpointPrefix<'a> = Option<&'a str>;

pub struct ContainerRegistryInfo {
    get_registry_endpoint: Box<dyn Fn(RegistryEndpointPrefix) -> Url + Send + Sync>,
    get_registry_url_prefix: Box<dyn Fn(QoveryIdentifier) -> Option<String> + Send + Sync>,
    // Contains username and password if necessary
    pub registry_name: String,
    get_registry_docker_json_config: Box<dyn Fn(DockerRegistryInfo) -> Option<String> + Send + Sync>,
    pub insecure_registry: bool,

    // this one is deprecated in favor of shared one, but we still need it for registry deletion purpose
    // give it the name of your image, and it returns the full name with prefix if needed
    // i.e: fo scaleway => image_name/image_name
    // i.e: for AWS => image_name
    get_image_name: Box<dyn Fn(&str) -> String + Send + Sync>,
    // get_mirrored_image_name: Box<dyn Fn(&Uuid, &Uuid, &RegistryMirroringMode) -> String + Send + Sync>,
    get_shared_image_name: Box<dyn Fn(&ImageBuildContext) -> String + Send + Sync>,

    // this one is deprecated in favor of shared one, but we still need it for registry deletion purpose
    // Give it the name of your image, and it returns the name of the repository that will be used
    get_repository_name: Box<dyn Fn(&str) -> String + Send + Sync>,
    get_shared_repository_name: Box<dyn Fn(&ImageBuildContext) -> String + Send + Sync>,
}

impl ContainerRegistryInfo {
    pub fn get_registry_docker_json_config(&self, docker_registry_info: DockerRegistryInfo) -> Option<String> {
        (self.get_registry_docker_json_config)(docker_registry_info)
    }

    pub fn get_registry_url_prefix(&self, cluster_id: QoveryIdentifier) -> Option<String> {
        (self.get_registry_url_prefix)(cluster_id)
    }

    pub fn get_repository_name(&self, image_name: &str) -> String {
        (self.get_repository_name)(image_name)
    }

    pub fn get_shared_repository_name(&self, cluster_id: &QoveryIdentifier, git_repo_url_sanitized: String) -> String {
        (self.get_shared_repository_name)(&ImageBuildContext {
            cluster_id: cluster_id.clone(),
            git_repo_url_sanitized,
        })
    }

    pub fn get_image_name(&self, image_name: &str) -> String {
        (self.get_image_name)(image_name)
    }

    pub fn get_shared_image_name(&self, cluster_id: &QoveryIdentifier, git_repo_url_sanitized: String) -> String {
        (self.get_shared_image_name)(&ImageBuildContext {
            cluster_id: cluster_id.clone(),
            git_repo_url_sanitized,
        })
    }

    pub fn get_registry_endpoint(&self, registry_url_prefix: Option<&str>) -> Url {
        (self.get_registry_endpoint)(registry_url_prefix)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Kind {
    Ecr,
    AzureContainerRegistry,
    ScalewayCr,
    GcpArtifactRegistry,
    GenericCr,
    GithubCr,
}

#[derive(Clone, PartialEq, Debug)]
pub struct RepositoryInfo {
    pub created: bool,
}

fn take_last_x_chars_and_remove_leading_dash_char(input: &str, max_length: usize) -> String {
    let truncated = take_last_x_chars(input, max_length);
    match truncated.chars().next() {
        Some('-') => truncated.chars().skip(1).collect(),
        _ => truncated,
    }
}

fn take_last_x_chars(input: &str, max_length: usize) -> String {
    let length_to_skip = input.len().saturating_sub(max_length);
    input.chars().skip(length_to_skip).collect()
}

#[cfg(test)]
mod test {
    use crate::infrastructure::models::container_registry::take_last_x_chars_and_remove_leading_dash_char;

    #[test]
    fn when_string_is_starting_by_dash_remove_it() {
        let result = take_last_x_chars_and_remove_leading_dash_char("-test", 5);
        assert_eq!(result, "test");
    }

    #[test]
    fn when_string_has_inner_dash_remove_it() {
        let result = take_last_x_chars_and_remove_leading_dash_char("removed-test", 5);
        assert_eq!(result, "test");
    }

    #[test]
    fn when_string_has_no_dash_dont_remove_anything() {
        let result = take_last_x_chars_and_remove_leading_dash_char("totest", 4);
        assert_eq!(result, "test");
    }
}

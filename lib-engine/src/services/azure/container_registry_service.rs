use crate::cmd::command::{CommandKiller, ExecutableCommand, QoveryCommand};
use crate::environment::models::ToCloudProviderFormat;
use crate::infrastructure::models::cloud_provider::azure::locations::AzureLocation;
use crate::infrastructure::models::container_registry::{DockerImage, Repository};
use crate::runtime::block_on;
use crate::services::azure::azure_cloud_sdk_types::{DockerImageTag, from_azure_container_registry};
use azure_core::authority_hosts::AZURE_PUBLIC_CLOUD;
use azure_core::new_http_client;
use azure_identity::ClientSecretCredential;
use azure_mgmt_containerregistry::models::{Registry, Resource, Sku};
use azure_mgmt_containerregistry::{Client, ClientBuilder};
use chrono::Duration;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration as StdDuration;
use thiserror::Error;

#[derive(Clone, Error, Debug, PartialEq, Eq)]
pub enum ContainerRegistryServiceError {
    #[error("Cannot create container registry service: {raw_error_message:?}")]
    CannotCreateService { raw_error_message: String },
    #[error("Cannot proceed, admission control blocked after several tries")]
    AdmissionControlCannotProceedAfterSeveralTries,
    #[error("Cannot get repository `{repository_name}`: {raw_error_message:?}")]
    CannotGetRepository {
        repository_name: String,
        raw_error_message: String,
    },
    #[error("Cannot create repository `{repository_name}`: {raw_error_message:?}")]
    CannotCreateRepository {
        repository_name: String,
        raw_error_message: String,
    },
    #[error("Cannot delete repository `{repository_name}`: {raw_error_message:?}")]
    CannotDeleteRepository {
        repository_name: String,
        raw_error_message: String,
    },
    #[error("Cannot get Docker image `{repository_name}/{image_name}@{image_tag}`: {raw_error_message:?}")]
    CannotGetDockerImage {
        repository_name: String,
        image_name: String,
        image_tag: String,
        raw_error_message: String,
    },
    #[error("Cannot list Docker images from `{repository_name}`: {raw_error_message:?}")]
    CannotListDockerImages {
        repository_name: String,
        raw_error_message: String,
    },
    #[error("Cannot delete Docker image `{repository_name}/{image_name}@{image_tag}`: {raw_error_message:?}")]
    CannotDeleteDockerImage {
        repository_name: String,
        image_name: String,
        image_tag: String,
        raw_error_message: String,
    },
}

pub const MAX_REGISTRY_NAME_LENGTH: usize = 50;
pub const MIN_REGISTRY_NAME_LENGTH: usize = 5;

pub struct AzureContainerRegistryService {
    client: Arc<Client>,
    client_id: String,
    client_secret: String,
}

impl AzureContainerRegistryService {
    pub fn new(tenant_id: &str, client_id: &str, client_secret: &str) -> Result<Self, ContainerRegistryServiceError> {
        let credentials = Arc::new(ClientSecretCredential::new(
            new_http_client(),
            AZURE_PUBLIC_CLOUD.clone(),
            tenant_id.to_string(),
            client_id.to_string(),
            client_secret.to_string(),
        ));

        let client = ClientBuilder::new(credentials.clone()).build().map_err(|e| {
            ContainerRegistryServiceError::CannotCreateService {
                raw_error_message: e.to_string(),
            }
        })?;

        Ok(AzureContainerRegistryService {
            client: Arc::new(client),
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
        })
    }

    pub fn get_registry(
        &self,
        subscription_id: &str,
        resource_group_name: &str,
        registry_name: &str,
    ) -> Result<Repository, ContainerRegistryServiceError> {
        let registry = block_on(
            self.client
                .registries_client()
                .get(subscription_id, resource_group_name, registry_name)
                .into_future(),
        )
        .map_err(|e| ContainerRegistryServiceError::CannotGetRepository {
            repository_name: registry_name.to_string(),
            raw_error_message: e.to_string(),
        })?;

        from_azure_container_registry(registry).map_err(|e| ContainerRegistryServiceError::CannotCreateRepository {
            repository_name: registry_name.to_string(),
            raw_error_message: e.to_string(),
        })
    }

    pub fn create_registry(
        &self,
        subscription_id: &str,
        resource_group_name: &str,
        location: AzureLocation,
        registry_name: &str,
        registry_sku: Sku, // https://learn.microsoft.com/en-us/azure/container-registry/container-registry-skus
        _image_retention_time: Option<Duration>, // TODO(benjaminch): Add image retention time
        _labels: Option<HashMap<String, String>>, // TODO(benjaminch): Add labels
    ) -> Result<Repository, ContainerRegistryServiceError> {
        // Checking registry name validity
        // Resource names may contain alpha numeric characters only and must be between 5 and 50 characters..
        if (registry_name.len() < MIN_REGISTRY_NAME_LENGTH || registry_name.len() > MAX_REGISTRY_NAME_LENGTH)
            || !registry_name.chars().all(char::is_alphanumeric)
        {
            return Err(ContainerRegistryServiceError::CannotCreateRepository {
                repository_name: registry_name.to_string(),
                raw_error_message: format!(
                    "Registry name must contain alpha numeric characters only and be between {} and {} characters",
                    MIN_REGISTRY_NAME_LENGTH, MAX_REGISTRY_NAME_LENGTH
                ),
            });
        }

        let registry = block_on(
            self.client
                .registries_client()
                .create(
                    subscription_id,
                    resource_group_name,
                    registry_name,
                    Registry::new(Resource::new(location.to_cloud_provider_format().to_string()), registry_sku),
                )
                .into_future(),
        )
        .map_err(|e| ContainerRegistryServiceError::CannotCreateRepository {
            repository_name: registry_name.to_string(),
            raw_error_message: e.to_string(),
        })?;

        // TODO(benjaminch): Add labels to the registry

        from_azure_container_registry(registry).map_err(|e| ContainerRegistryServiceError::CannotCreateRepository {
            repository_name: registry_name.to_string(),
            raw_error_message: e.to_string(),
        })
    }

    pub fn delete_registry(
        &self,
        subscription_id: &str,
        resource_group_name: &str,
        registry_name: &str,
    ) -> Result<(), ContainerRegistryServiceError> {
        block_on(
            self.client
                .registries_client()
                .delete(subscription_id, resource_group_name, registry_name)
                .send(),
        )
        .map_err(|e| ContainerRegistryServiceError::CannotDeleteRepository {
            repository_name: registry_name.to_string(),
            raw_error_message: e.to_string(),
        })?;

        Ok(())
    }

    pub fn get_docker_image(
        &self,
        _subscription_id: &str,
        _resource_group_name: &str,
        registry_name: &str,
        image_name: &str,
        image_tag: &str,
    ) -> Result<DockerImage, ContainerRegistryServiceError> {
        // TODO(benjaminch): move out azure CLI once repository operations will be available in SDK
        // https://crates.io/crates/azure_containers_containerregistry
        let mut output = vec![];
        let mut error = vec![];
        QoveryCommand::new(
            "az",
            &[
                "acr",
                "repository",
                "show",
                "-n",
                registry_name,
                "-o",
                "json",
                "--image",
                format!("{}:{}", image_name, image_tag).as_str(),
                "-u",
                self.client_id.as_str(),
                "-p",
                self.client_secret.as_str(),
            ],
            &[],
        )
        .exec_with_abort(
            &mut |line| {
                output.push(line);
            },
            &mut |line| {
                error.push(line);
            },
            &CommandKiller::from_timeout(StdDuration::from_secs(30)),
        )
        .map_err(|e| ContainerRegistryServiceError::CannotGetDockerImage {
            repository_name: registry_name.to_string(),
            image_name: image_name.to_string(),
            image_tag: image_tag.to_string(),
            raw_error_message: format!("Cannot get docker image: {}", e),
        })?;

        let docker_image_tag = serde_json::from_str::<DockerImageTag>(output.join("").as_str()).map_err(|e| {
            ContainerRegistryServiceError::CannotGetDockerImage {
                repository_name: registry_name.to_string(),
                image_name: image_name.to_string(),
                image_tag: image_tag.to_string(),
                raw_error_message: format!("Cannot parse docker image tag: {}", e),
            }
        })?;

        Ok(DockerImage {
            repository_id: registry_name.to_string(),
            name: image_name.to_string(),
            tag: docker_image_tag.name,
        })
    }

    pub fn list_docker_images(
        &self,
        _subscription_id: &str,
        _resource_group_name: &str,
        registry_name: &str,
    ) -> Result<Vec<DockerImage>, ContainerRegistryServiceError> {
        // TODO(benjaminch): move out azure CLI once repository operations will be available in SDK
        // https://crates.io/crates/azure_containers_containerregistry
        let mut output = vec![];
        let mut error = vec![];
        QoveryCommand::new(
            "az",
            &[
                "acr",
                "repository",
                "list",
                "-n",
                registry_name,
                "-o",
                "json",
                "-u",
                self.client_id.as_str(),
                "-p",
                self.client_secret.as_str(),
            ],
            &[],
        )
        .exec_with_abort(
            &mut |line| {
                output.push(line);
            },
            &mut |line| {
                error.push(line);
            },
            &CommandKiller::from_timeout(StdDuration::from_secs(30)),
        )
        .map_err(|e| ContainerRegistryServiceError::CannotListDockerImages {
            repository_name: registry_name.to_string(),
            raw_error_message: format!("Cannot list docker images: {}", e),
        })?;

        let docker_image = serde_json::from_str::<Vec<String>>(output.join("").as_str()).map_err(|e| {
            ContainerRegistryServiceError::CannotListDockerImages {
                repository_name: registry_name.to_string(),
                raw_error_message: format!("Cannot parse docker images: {}", e),
            }
        })?;

        let mut docker_images = vec![];

        for image in docker_image {
            docker_images.push(DockerImage {
                repository_id: registry_name.to_string(),
                name: image.clone(),
                tag: "".to_string(), // TODO(benjaminch): Add tag to the image
            });
        }

        Ok(docker_images)
    }
}

use crate::cmd::command::{CommandKiller, ExecutableCommand, QoveryCommand};
use crate::environment::models::ToCloudProviderFormat;
use crate::infrastructure::models::cloud_provider::azure::locations::AzureLocation;
use crate::infrastructure::models::container_registry::{DockerImage, Repository};
use crate::runtime::block_on;
use crate::services::azure::azure_auth_service::AzureAuthService;
use crate::services::azure::azure_cloud_sdk_types::{DockerImageTag, from_azure_container_registry};
use azure_core::authority_hosts::AZURE_PUBLIC_CLOUD;
use azure_core::new_http_client;
use azure_identity::ClientSecretCredential;
use azure_mgmt_containerregistry::models::{
    Policies, Registry, RegistryPropertiesUpdateParameters, RegistryUpdateParameters, Resource, Sku,
};
use azure_mgmt_containerregistry::{Client, ClientBuilder};
use chrono::Duration;
use governor::middleware::NoOpMiddleware;
use governor::state::{InMemoryState, NotKeyed};
use governor::{RateLimiter, clock};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration as StdDuration, Instant};
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
    #[error("Invalid registry name: {raw_error_message:?}")]
    InvalidRegistryName {
        registry_name: String,
        raw_error_message: String,
    },
    #[error(
        "Error while trying to allow cluster `{cluster_name}` to pull from `{registry_name}`: {raw_error_message:?}"
    )]
    CannotAllowClusterToPullFromRegistry {
        registry_name: String,
        cluster_name: String,
        raw_error_message: String,
    },
    #[error("Cannot login to Azure registry `{registry_name}`: {raw_error_message:?}")]
    CannotLoginToRegistry {
        registry_name: String,
        raw_error_message: String,
    },
}

pub const MAX_REGISTRY_NAME_LENGTH: usize = 50;
pub const MIN_REGISTRY_NAME_LENGTH: usize = 5;

enum RateLimiterKind {
    Write,
    Read,
}

pub struct AzureContainerRegistryService {
    client: Arc<Client>,
    client_id: String,
    client_secret: String,
    tenant_id: String,
    write_rate_limiter: Option<Arc<RateLimiter<NotKeyed, InMemoryState, clock::DefaultClock, NoOpMiddleware>>>,
    read_rate_limiter: Option<Arc<RateLimiter<NotKeyed, InMemoryState, clock::DefaultClock, NoOpMiddleware>>>,
}

impl AzureContainerRegistryService {
    pub fn new(
        tenant_id: &str,
        client_id: &str,
        client_secret: &str,
        write_rate_limiter: Option<Arc<RateLimiter<NotKeyed, InMemoryState, clock::DefaultClock, NoOpMiddleware>>>,
        read_rate_limiter: Option<Arc<RateLimiter<NotKeyed, InMemoryState, clock::DefaultClock, NoOpMiddleware>>>,
    ) -> Result<Self, ContainerRegistryServiceError> {
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
            tenant_id: tenant_id.to_string(),
            write_rate_limiter,
            read_rate_limiter,
        })
    }

    fn wait_for_a_slot_in_admission_control(
        &self,
        rate_limiter_kind: RateLimiterKind,
        timeout: std::time::Duration,
    ) -> Result<(), ContainerRegistryServiceError> {
        if let Some(rate_limiter) = match rate_limiter_kind {
            RateLimiterKind::Write => &self.write_rate_limiter,
            RateLimiterKind::Read => &self.read_rate_limiter,
        } {
            let start = Instant::now();

            loop {
                if start.elapsed() > timeout {
                    return Err(ContainerRegistryServiceError::AdmissionControlCannotProceedAfterSeveralTries);
                }

                if rate_limiter.check().is_err() {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    continue;
                }

                break;
            }
        }

        Ok(())
    }

    pub fn try_get_sanitized_registry_name(registry_name: &str) -> Result<String, ContainerRegistryServiceError> {
        // If registry name has less than minimum length, we don't pad, just return an error
        if registry_name.len() < MIN_REGISTRY_NAME_LENGTH {
            return Err(ContainerRegistryServiceError::InvalidRegistryName {
                registry_name: registry_name.to_string(),
                raw_error_message: format!(
                    "Registry name must contain alpha numeric characters only and be at least {} characters",
                    MIN_REGISTRY_NAME_LENGTH
                ),
            });
        }

        Ok(registry_name
            .chars()
            .filter(|c| c.is_alphanumeric()) // Replace all non-alphanumeric characters with an empty string
            .take(MAX_REGISTRY_NAME_LENGTH) // Trimming the registry name if over 50 characters
            .collect::<String>())
    }

    pub fn allow_cluster_to_pull_from_registry(
        &self,
        resource_group_name: &str,
        registry_name: &str,
        cluster_name: &str,
    ) -> Result<(), ContainerRegistryServiceError> {
        let registry_name = Self::try_get_sanitized_registry_name(registry_name)?;
        AzureAuthService::login(&self.client_id, &self.client_secret, &self.tenant_id).map_err(|e| {
            ContainerRegistryServiceError::CannotAllowClusterToPullFromRegistry {
                cluster_name: cluster_name.to_string(),
                registry_name: registry_name.to_string(),
                raw_error_message: format!("Cannot login to Azure to allow cluster to pull from ACR: {}", e),
            }
        })?;

        // Link the ACR to the AKS cluster
        // This can fail especially if cluster is not yet ready or if there are any updates
        // operations in progress on the cluster.
        // Example error message:
        // Operation is not allowed because there's an in progress update managed cluster operation
        let mut output = vec![];
        let mut error_output = vec![];
        match retry::retry(retry::delay::Fixed::from_millis(15_000).take(20), || {
            // az aks update -n <myAKSCluster> -g <myResourceGroup> --attach-acr <acr-resource-id>
            match QoveryCommand::new(
                "az",
                &[
                    "aks",
                    "update",
                    "-n",
                    cluster_name,
                    "-g",
                    resource_group_name,
                    "--attach-acr",
                    registry_name.as_str(),
                    "--no-wait",
                ],
                &[],
            )
            .exec_with_abort(
                &mut |line| {
                    output.push(line);
                },
                &mut |line| {
                    error_output.push(line);
                },
                &CommandKiller::from_timeout(StdDuration::from_secs(10 * 60)),
            ) {
                Ok(_) => retry::OperationResult::Ok(()),
                Err(e) => retry::OperationResult::Retry(e),
            }
        }) {
            Ok(_) => Ok(()),
            Err(retry::Error { error, .. }) => {
                Err(ContainerRegistryServiceError::CannotAllowClusterToPullFromRegistry {
                    cluster_name: cluster_name.to_string(),
                    registry_name: registry_name.to_string(),
                    raw_error_message: format!(
                        "Cannot allow cluster to pull from ACR: {}\n\n{}\n\n{}",
                        error,
                        output.join("").as_str(),
                        error_output.join("").as_str()
                    ),
                })
            }
        }
    }

    pub fn get_registry(
        &self,
        subscription_id: &str,
        resource_group_name: &str,
        registry_name: &str,
    ) -> Result<Repository, ContainerRegistryServiceError> {
        self.wait_for_a_slot_in_admission_control(RateLimiterKind::Read, std::time::Duration::from_secs(60))?;

        let registry_name = Self::try_get_sanitized_registry_name(registry_name)?;

        let registry = block_on(
            self.client
                .registries_client()
                .get(subscription_id, resource_group_name, registry_name.to_string())
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
        image_retention_time: Option<Duration>,
        labels: Option<HashMap<String, String>>,
    ) -> Result<Repository, ContainerRegistryServiceError> {
        self.wait_for_a_slot_in_admission_control(RateLimiterKind::Write, std::time::Duration::from_secs(60))?;

        let registry_name = Self::try_get_sanitized_registry_name(registry_name)?;

        let registry = block_on(
            self.client
                .registries_client()
                .create(
                    subscription_id,
                    resource_group_name,
                    registry_name.to_string(),
                    Registry::new(
                        Resource::new(location.to_cloud_provider_format().to_string()),
                        registry_sku.clone(),
                    ),
                )
                .into_future(),
        )
        .map_err(|e| ContainerRegistryServiceError::CannotCreateRepository {
            repository_name: registry_name.to_string(),
            raw_error_message: e.to_string(),
        })?;

        let mut retention_policies = registry
            .properties
            .unwrap_or_default()
            .policies
            .unwrap_or_default()
            .retention_policy
            .unwrap_or_default();
        retention_policies.days = image_retention_time.map(|d| d.num_days() as i32);

        self.wait_for_a_slot_in_admission_control(RateLimiterKind::Write, std::time::Duration::from_secs(60))?;

        let registry = block_on(
            self.client
                .registries_client()
                .update(
                    subscription_id,
                    resource_group_name,
                    registry_name.to_string(),
                    RegistryUpdateParameters {
                        identity: None,
                        sku: Some(registry_sku),
                        tags: match &labels {
                            Some(labels) => Some(serde_json::to_value(labels).map_err(|e| {
                                ContainerRegistryServiceError::CannotCreateRepository {
                                    repository_name: registry_name.to_string(),
                                    raw_error_message: e.to_string(),
                                }
                            })?),
                            None => None,
                        },
                        properties: Some(RegistryPropertiesUpdateParameters {
                            policies: Some(Policies {
                                retention_policy: Some(retention_policies),
                                ..Default::default()
                            }),
                            ..Default::default()
                        }),
                    },
                )
                .into_future(),
        )
        .map_err(|e| ContainerRegistryServiceError::CannotCreateRepository {
            repository_name: registry_name.to_string(),
            raw_error_message: e.to_string(),
        })?;

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
        self.wait_for_a_slot_in_admission_control(RateLimiterKind::Write, std::time::Duration::from_secs(60))?;

        let registry_name = Self::try_get_sanitized_registry_name(registry_name)?;
        block_on(
            self.client
                .registries_client()
                .delete(subscription_id, resource_group_name, registry_name.to_string())
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
        self.wait_for_a_slot_in_admission_control(RateLimiterKind::Read, std::time::Duration::from_secs(60))?;

        let sanitized_registry_name = Self::try_get_sanitized_registry_name(registry_name)?;
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
                sanitized_registry_name.as_str(),
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
            repository_name: sanitized_registry_name.to_string(),
            image_name: image_name.to_string(),
            image_tag: image_tag.to_string(),
            raw_error_message: format!("Cannot get docker image: {}", e),
        })?;

        let docker_image_tag = serde_json::from_str::<DockerImageTag>(output.join("").as_str()).map_err(|e| {
            ContainerRegistryServiceError::CannotGetDockerImage {
                repository_name: sanitized_registry_name.to_string(),
                image_name: image_name.to_string(),
                image_tag: image_tag.to_string(),
                raw_error_message: format!("Cannot parse docker image tag: {}", e),
            }
        })?;

        Ok(DockerImage {
            repository_id: sanitized_registry_name.to_string(),
            name: image_name.to_string(),
            tag: docker_image_tag.name,
        })
    }

    pub fn delete_docker_image(
        &self,
        _subscription_id: &str,
        _resource_group_name: &str,
        registry_name: &str,
        image_name: &str,
        image_tag: &str,
    ) -> Result<(), ContainerRegistryServiceError> {
        self.wait_for_a_slot_in_admission_control(RateLimiterKind::Write, std::time::Duration::from_secs(60))?;

        let registry_name = Self::try_get_sanitized_registry_name(registry_name)?;
        // TODO(benjaminch): move out azure CLI once repository operations will be available in SDK
        // https://crates.io/crates/azure_containers_containerregistry
        QoveryCommand::new(
            "az",
            &[
                "acr",
                "repository",
                "delete",
                "-n",
                registry_name.as_str(),
                "--image",
                format!("{}:{}", image_name, image_tag).as_str(),
                "--yes",
                "-u",
                self.client_id.as_str(),
                "-p",
                self.client_secret.as_str(),
            ],
            &[],
        )
        .exec_with_abort(
            &mut |_line| {},
            &mut |_line| {},
            &CommandKiller::from_timeout(StdDuration::from_secs(60)),
        )
        .map_err(|e| ContainerRegistryServiceError::CannotDeleteDockerImage {
            repository_name: registry_name.to_string(),
            image_name: image_name.to_string(),
            image_tag: image_tag.to_string(),
            raw_error_message: format!("Cannot delete docker image: {}", e),
        })?;

        Ok(())
    }

    pub fn list_docker_images(
        &self,
        _subscription_id: &str,
        _resource_group_name: &str,
        registry_name: &str,
    ) -> Result<Vec<DockerImage>, ContainerRegistryServiceError> {
        self.wait_for_a_slot_in_admission_control(RateLimiterKind::Read, std::time::Duration::from_secs(60))?;

        let registry_name = Self::try_get_sanitized_registry_name(registry_name)?;

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
                registry_name.as_str(),
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

#[cfg(test)]
mod tests {
    use crate::services::azure::container_registry_service::ContainerRegistryServiceError;

    #[test]
    fn test_azure_container_registry_service_try_get_sanitized_registry_name() {
        let registry_name = "myregistry";
        let sanitized_registry_name =
            super::AzureContainerRegistryService::try_get_sanitized_registry_name(registry_name);
        assert_eq!(sanitized_registry_name, Ok("myregistry".to_string()));

        let registry_name = "my-registry";
        let sanitized_registry_name =
            super::AzureContainerRegistryService::try_get_sanitized_registry_name(registry_name);
        assert_eq!(sanitized_registry_name, Ok("myregistry".to_string()));

        let registry_name = "my_registry";
        let sanitized_registry_name =
            super::AzureContainerRegistryService::try_get_sanitized_registry_name(registry_name);
        assert_eq!(sanitized_registry_name, Ok("myregistry".to_string()));

        let registry_name = " my registry ";
        let sanitized_registry_name =
            super::AzureContainerRegistryService::try_get_sanitized_registry_name(registry_name);
        assert_eq!(sanitized_registry_name, Ok("myregistry".to_string()));

        let registry_name = "myregistry12345678901234567890123456789012345678901234567890";
        let sanitized_registry_name =
            super::AzureContainerRegistryService::try_get_sanitized_registry_name(registry_name);
        assert_eq!(
            sanitized_registry_name,
            Ok("myregistry1234567890123456789012345678901234567890".to_string())
        );

        let registry_name = "reg";
        let sanitized_registry_name =
            super::AzureContainerRegistryService::try_get_sanitized_registry_name(registry_name);
        assert_eq!(
            sanitized_registry_name,
            Err(ContainerRegistryServiceError::InvalidRegistryName {
                registry_name: "reg".to_string(),
                raw_error_message:
                    "Registry name must contain alpha numeric characters only and be at least 5 characters".to_string(),
            }),
        );
    }
}

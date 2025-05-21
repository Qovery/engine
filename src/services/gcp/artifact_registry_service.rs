use crate::environment::models::ToCloudProviderFormat;
use crate::environment::models::gcp::JsonCredentials;
use crate::infrastructure::models::cloud_provider::gcp::locations::GcpRegion;
use crate::infrastructure::models::container_registry::{DockerImage, Repository};
use crate::runtime::block_on;
use crate::services::gcp::google_cloud_sdk_types::{from_gcp_repository, new_gcp_credentials_file_from_credentials};
use google_cloud_artifact_registry::client::{Client, ClientConfig};
use google_cloud_googleapis::devtools::artifact_registry::v1::repository::Format;
use google_cloud_googleapis::devtools::artifact_registry::v1::{
    CreateRepositoryRequest, DeletePackageRequest, DeleteRepositoryRequest, GetRepositoryRequest,
    ListDockerImagesRequest, Repository as GcpRepository,
};
use governor::middleware::NoOpMiddleware;
use governor::state::{InMemoryState, NotKeyed};
use governor::{RateLimiter, clock};
use std::borrow::BorrowMut;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Clone, Error, Debug, PartialEq, Eq)]
pub enum ArtifactRegistryServiceError {
    #[error("Cannot create artifact registry service: {raw_error_message:?}")]
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
    #[error("Cannot delete Docker image `{repository_name}/{image_name}@{image_tag}`: {raw_error_message:?}")]
    CannotDeleteDockerImage {
        repository_name: String,
        image_name: String,
        image_tag: String,
        raw_error_message: String,
    },
}

enum ArtifactRegistryResourceKind {
    Repository,
    Image,
}

pub struct ArtifactRegistryService {
    client: Arc<Mutex<Client>>,
    write_repository_rate_limiter:
        Option<Arc<RateLimiter<NotKeyed, InMemoryState, clock::DefaultClock, NoOpMiddleware>>>,
    write_image_rate_limiter: Option<Arc<RateLimiter<NotKeyed, InMemoryState, clock::DefaultClock, NoOpMiddleware>>>,
}

impl ArtifactRegistryService {
    pub fn new(
        google_credentials: JsonCredentials,
        write_repository_rate_limiter: Option<
            Arc<RateLimiter<NotKeyed, InMemoryState, clock::DefaultClock, NoOpMiddleware>>,
        >,
        write_image_rate_limiter: Option<
            Arc<RateLimiter<NotKeyed, InMemoryState, clock::DefaultClock, NoOpMiddleware>>,
        >,
    ) -> Result<Self, ArtifactRegistryServiceError> {
        Ok(Self {
            client: Arc::new(Mutex::from(
                block_on(Client::new(
                    block_on(ClientConfig::default().with_credentials(
                        new_gcp_credentials_file_from_credentials(google_credentials).map_err(|e| {
                            ArtifactRegistryServiceError::CannotCreateService {
                                raw_error_message: e.to_string(),
                            }
                        })?,
                    ))
                    .map_err(|e| ArtifactRegistryServiceError::CannotCreateService {
                        raw_error_message: e.to_string(),
                    })?,
                ))
                .map_err(|e| ArtifactRegistryServiceError::CannotCreateService {
                    raw_error_message: e.to_string(),
                })?,
            )),
            write_repository_rate_limiter,
            write_image_rate_limiter,
        })
    }

    fn wait_for_a_slot_in_admission_control(
        &self,
        timeout: std::time::Duration,
        resource_kind: ArtifactRegistryResourceKind,
    ) -> Result<(), ArtifactRegistryServiceError> {
        if let Some(rate_limiter) = match resource_kind {
            ArtifactRegistryResourceKind::Repository => &self.write_repository_rate_limiter,
            ArtifactRegistryResourceKind::Image => &self.write_image_rate_limiter,
        } {
            let start = Instant::now();

            loop {
                if start.elapsed() > timeout {
                    return Err(ArtifactRegistryServiceError::AdmissionControlCannotProceedAfterSeveralTries);
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

    pub fn get_repository(
        &self,
        project_id: &str,
        location: GcpRegion,
        repository_name: &str,
    ) -> Result<Repository, ArtifactRegistryServiceError> {
        let repository_identifier = format!(
            "projects/{}/locations/{}/repositories/{}",
            project_id,
            location.to_cloud_provider_format(),
            repository_name
        );

        let gcp_repository: GcpRepository =
            block_on(self.client.clone().blocking_lock_owned().borrow_mut().get_repository(
                GetRepositoryRequest {
                    name: repository_identifier.to_string(),
                },
                None,
            ))
            .map_err(|e| ArtifactRegistryServiceError::CannotGetRepository {
                repository_name: repository_identifier.to_string(),
                raw_error_message: e.to_string(),
            })?;

        from_gcp_repository(project_id, location, gcp_repository).map_err(|e| {
            ArtifactRegistryServiceError::CannotGetRepository {
                repository_name: repository_identifier.to_string(),
                raw_error_message: e.to_string(),
            }
        })
    }

    pub fn create_repository(
        &self,
        project_id: &str,
        location: GcpRegion,
        repository_name: &str,
        labels: HashMap<String, String>,
    ) -> Result<Repository, ArtifactRegistryServiceError> {
        self.wait_for_a_slot_in_admission_control(
            std::time::Duration::from_secs(10 * 60),
            ArtifactRegistryResourceKind::Repository,
        )?;

        let gcp_repository = match block_on(
            block_on(
                self.client
                    .clone()
                    .blocking_lock_owned()
                    .borrow_mut()
                    .create_repository(
                        // TODO(ENG-1808): add repository TTL
                        CreateRepositoryRequest {
                            parent: format!(
                                "projects/{}/locations/{}",
                                project_id,
                                location.to_cloud_provider_format(),
                            ),
                            repository_id: repository_name.to_string(),
                            repository: Some(GcpRepository {
                                name: repository_name.to_string(),
                                format: Format::Docker.into(),
                                labels,
                                ..Default::default()
                            }),
                        },
                        None,
                    ),
            )
            .as_mut()
            .map_err(|e| ArtifactRegistryServiceError::CannotCreateRepository {
                repository_name: repository_name.to_string(),
                raw_error_message: e.to_string(),
            })?
            .wait(None),
        )
        .map_err(|e| ArtifactRegistryServiceError::CannotCreateRepository {
            repository_name: repository_name.to_string(),
            raw_error_message: e.to_string(),
        })? {
            Some(r) => r,
            None => {
                return Err(ArtifactRegistryServiceError::CannotGetRepository {
                    repository_name: repository_name.to_string(),
                    raw_error_message: "Operation returned an empty repository".to_string(),
                });
            }
        };

        from_gcp_repository(project_id, location, gcp_repository).map_err(|e| {
            ArtifactRegistryServiceError::CannotGetRepository {
                repository_name: repository_name.to_string(),
                raw_error_message: e.to_string(),
            }
        })
    }

    pub fn delete_repository(
        &self,
        project_id: &str,
        location: GcpRegion,
        repository_name: &str,
    ) -> Result<(), ArtifactRegistryServiceError> {
        self.wait_for_a_slot_in_admission_control(
            std::time::Duration::from_secs(10 * 60),
            ArtifactRegistryResourceKind::Repository,
        )?;

        let repository_identifier = format!(
            "projects/{}/locations/{}/repositories/{}",
            project_id,
            location.to_cloud_provider_format(),
            repository_name
        );

        let delete_repository_result = block_on(
            self.client
                .clone()
                .blocking_lock_owned()
                .borrow_mut()
                .delete_repository(
                    DeleteRepositoryRequest {
                        name: repository_identifier.to_string(),
                    },
                    None,
                ),
        );
        match delete_repository_result {
            Ok(_) => {}
            Err(status) => {
                if status.code() != google_cloud_gax::grpc::Code::NotFound {
                    return Err(ArtifactRegistryServiceError::CannotDeleteRepository {
                        repository_name: repository_identifier,
                        raw_error_message: status.to_string(),
                    });
                }
            }
        }

        Ok(())
    }

    pub fn get_docker_image(
        &self,
        project_id: &str,
        location: GcpRegion,
        repository_name: &str,
        image_name: &str,
        image_tag: &str,
    ) -> Result<DockerImage, ArtifactRegistryServiceError> {
        // Seems we cannot properly retrieve an image per tag, can be only done via digest ...
        // to be investigated, also package object can be used here
        let docker_image_identifier = format!(
            "projects/{}/locations/{}/repositories/{}/dockerImages/{}",
            project_id,
            location.to_cloud_provider_format(),
            repository_name,
            image_name,
        );

        let mut next_page_token: String = "".to_string();

        loop {
            // list all images for the repository, trying to find the requested image having the requested tag
            match block_on(
                self.client
                    .clone()
                    .blocking_lock_owned()
                    .borrow_mut()
                    .list_docker_images(
                        ListDockerImagesRequest {
                            parent: format!(
                                "projects/{}/locations/{}/repositories/{}",
                                project_id,
                                location.to_cloud_provider_format(),
                                repository_name
                            ),
                            page_token: next_page_token.to_string(),
                            page_size: 100,
                            ..Default::default()
                        },
                        None,
                    ),
            ) {
                Ok(docker_images_list_response) => {
                    next_page_token = docker_images_list_response.next_page_token;
                    for docker_image in docker_images_list_response.docker_images {
                        // removing image sha, keeping only name / identifier part
                        let (remote_image_name, _remote_image_sha) =
                            docker_image.name.split_once("@sha256:").unwrap_or_default();
                        if remote_image_name == docker_image_identifier
                            && docker_image.tags.contains(&image_tag.to_string())
                        {
                            return DockerImage::try_from(docker_image).map_err(|e| {
                                ArtifactRegistryServiceError::CannotGetDockerImage {
                                    repository_name: repository_name.to_string(),
                                    image_name: image_name.to_string(),
                                    image_tag: image_tag.to_string(),
                                    raw_error_message: e.to_string(),
                                }
                            });
                        }
                    }

                    if next_page_token.is_empty() {
                        break;
                    }
                }
                Err(e) => {
                    return Err(ArtifactRegistryServiceError::CannotGetDockerImage {
                        repository_name: repository_name.to_string(),
                        image_name: image_name.to_string(),
                        image_tag: image_tag.to_string(),
                        raw_error_message: e.to_string(),
                    });
                }
            }
        }

        Err(ArtifactRegistryServiceError::CannotGetDockerImage {
            repository_name: repository_name.to_string(),
            image_name: image_name.to_string(),
            image_tag: image_tag.to_string(),
            raw_error_message: "No image found in the repository matching name and version".to_string(),
        })
    }

    pub fn delete_docker_image(
        &self,
        project_id: &str,
        location: GcpRegion,
        repository_name: &str,
        image_name: &str,
    ) -> Result<(), ArtifactRegistryServiceError> {
        self.wait_for_a_slot_in_admission_control(
            std::time::Duration::from_secs(10 * 60),
            ArtifactRegistryResourceKind::Image,
        )?;

        // Note: deleting the whole package here, not just the tag / version
        // if needed, deleting image tag only is doable
        block_on(self.client.clone().blocking_lock_owned().borrow_mut().delete_package(
            DeletePackageRequest {
                name: format!(
                    "projects/{}/locations/{}/repositories/{}/packages/{}",
                    project_id,
                    location.to_cloud_provider_format(),
                    repository_name,
                    image_name,
                ),
            },
            None,
        ))
        .map_err(|e| ArtifactRegistryServiceError::CannotDeleteDockerImage {
            repository_name: repository_name.to_string(),
            image_name: image_name.to_string(),
            image_tag: "".to_string(),
            raw_error_message: e.to_string(),
        })
    }
}

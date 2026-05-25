use crate::environment::models::ToCloudProviderFormat;
use crate::environment::models::gcp::JsonCredentials;
use crate::environment::models::gcp::io::JsonCredentials as IoJsonCredentials;
use crate::infrastructure::models::cloud_provider::gcp::locations::GcpRegion;
use crate::infrastructure::models::container_registry::{DockerImage, Repository};
use crate::runtime::block_on;
use crate::services::gcp::google_cloud_sdk_types::from_gcp_repository;
use google_cloud_artifactregistry_v1::client::ArtifactRegistry;
use google_cloud_artifactregistry_v1::model::Repository as GcpRepository;
use google_cloud_artifactregistry_v1::model::repository::Format;
use google_cloud_auth::credentials::service_account::Builder as ServiceAccountCredentialsBuilder;
use google_cloud_lro::Poller;
use governor::middleware::NoOpMiddleware;
use governor::state::{InMemoryState, NotKeyed};
use governor::{RateLimiter, clock};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use thiserror::Error;

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
    client: ArtifactRegistry,
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
        let service_account_json = serde_json::to_value(IoJsonCredentials::from(google_credentials)).map_err(|e| {
            ArtifactRegistryServiceError::CannotCreateService {
                raw_error_message: e.to_string(),
            }
        })?;

        let client = block_on(async move {
            let credentials = ServiceAccountCredentialsBuilder::new(service_account_json)
                .build()
                .map_err(|e| format!("Failed to build GCP service account credentials: {e}"))?;

            ArtifactRegistry::builder()
                .with_credentials(credentials)
                .build()
                .await
                .map_err(|e| format!("Failed to build Artifact Registry client: {e}"))
        })
        .map_err(|e| ArtifactRegistryServiceError::CannotCreateService { raw_error_message: e })?;

        Ok(Self {
            client,
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

        let gcp_repository: GcpRepository = block_on(
            self.client
                .get_repository()
                .set_name(repository_identifier.to_string())
                .send(),
        )
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

        let gcp_repository = block_on(
            self.client
                .create_repository()
                .set_parent(format!(
                    "projects/{}/locations/{}",
                    project_id,
                    location.to_cloud_provider_format(),
                ))
                .set_repository_id(repository_name.to_string())
                .set_repository(
                    GcpRepository::new()
                        .set_name(repository_name.to_string())
                        .set_format(Format::Docker)
                        .set_labels(labels),
                )
                .poller()
                .until_done(),
        )
        .map_err(|e| ArtifactRegistryServiceError::CannotCreateRepository {
            repository_name: repository_name.to_string(),
            raw_error_message: e.to_string(),
        })?;

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
                .delete_repository()
                .set_name(repository_identifier.to_string())
                .poller()
                .until_done(),
        );
        match delete_repository_result {
            Ok(_) => {}
            Err(status) => {
                if !is_not_found_error(&status) {
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
                    .list_docker_images()
                    .set_parent(format!(
                        "projects/{}/locations/{}/repositories/{}",
                        project_id,
                        location.to_cloud_provider_format(),
                        repository_name
                    ))
                    .set_page_token(next_page_token.to_string())
                    .set_page_size(100)
                    .send(),
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
        block_on(
            self.client
                .delete_package()
                .set_name(format!(
                    "projects/{}/locations/{}/repositories/{}/packages/{}",
                    project_id,
                    location.to_cloud_provider_format(),
                    repository_name,
                    image_name,
                ))
                .poller()
                .until_done(),
        )
        .map_err(|e| ArtifactRegistryServiceError::CannotDeleteDockerImage {
            repository_name: repository_name.to_string(),
            image_name: image_name.to_string(),
            image_tag: "".to_string(),
            raw_error_message: e.to_string(),
        })
    }
}

fn is_not_found_error(error: &google_cloud_artifactregistry_v1::Error) -> bool {
    if error.http_status_code() == Some(404) {
        return true;
    }

    let message = error.to_string().to_lowercase();
    message.contains("not_found") || message.contains("not found")
}

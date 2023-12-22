use crate::build_platform::Image;
use crate::cloud_provider::gcp::locations::GcpRegion;
use crate::container_registry::errors::ContainerRegistryError;
use crate::container_registry::{ContainerRegistry, ContainerRegistryInfo, Kind, Repository, RepositoryInfo};
use crate::io_models::context::Context;
use crate::models::gcp::io::JsonCredentials as JsonCredentialsIo;
use crate::models::gcp::JsonCredentials;
use crate::models::ToCloudProviderFormat;
use crate::services::gcp::artifact_registry_service::ArtifactRegistryService;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use url::Url;
use uuid::Uuid;

pub struct GoogleArtifactRegistry {
    context: Context,
    id: String,
    long_id: Uuid,
    name: String,
    project_id: String,
    region: GcpRegion,
    registry_info: ContainerRegistryInfo,
    service: Arc<ArtifactRegistryService>,
}

impl GoogleArtifactRegistry {
    pub fn new(
        context: Context,
        id: &str,
        long_id: Uuid,
        name: &str,
        project_id: &str,
        region: GcpRegion,
        credentials: JsonCredentials,
        service: Arc<ArtifactRegistryService>,
    ) -> Result<Self, ContainerRegistryError> {
        // Be sure we are logged on the registry
        let login = "_json_key".to_string();
        let secret_token = serde_json::to_string(&JsonCredentialsIo::from(credentials.clone())).map_err(|e| {
            ContainerRegistryError::CannotInstantiateClient {
                raw_error_message: e.to_string(),
            }
        })?;
        let registry_raw_url = format!("https://{}-docker.pkg.dev", region.to_cloud_provider_format(),);

        let mut registry =
            Url::parse(registry_raw_url.as_str()).map_err(|_e| ContainerRegistryError::InvalidRegistryUrl {
                registry_url: registry_raw_url,
            })?;
        let _ = registry.set_username(&login);
        let _ = registry.set_password(Some(&secret_token));

        if context
            .docker
            .login_artifact_registry(&registry, credentials.client_email.as_str(), &secret_token)
            .is_err()
        {
            return Err(ContainerRegistryError::InvalidCredentials);
        }

        let project_name = project_id.to_string();
        let registry_info = ContainerRegistryInfo {
            endpoint: registry,
            registry_name: name.to_string(),
            registry_docker_json_config: None,
            get_image_name: Box::new(move |img_name| format!("{}/{img_name}/{img_name}", &project_name.clone())),
            get_repository_name: Box::new(|repository_name| repository_name.to_string()),
        };

        Ok(Self {
            context,
            id: id.to_string(),
            long_id,
            name: name.to_string(),
            project_id: project_id.to_string(),
            region,
            registry_info,
            service,
        })
    }

    fn create_repository_without_exist_check(
        &self,
        repository_name: &str,
        _image_retention_time_in_seconds: u32,
        resource_ttl: Option<Duration>,
    ) -> Result<(Repository, RepositoryInfo), ContainerRegistryError> {
        let creation_date: DateTime<Utc> = Utc::now();
        self.service
            .create_repository(
                &self.project_id,
                self.region.clone(),
                repository_name,
                HashMap::from([
                    // Tags keys rule: Only hyphens (-), underscores (_), lowercase characters, and numbers are allowed.
                    // Keys must start with a lowercase character. International characters are allowed.
                    ("creation_date".to_string(), creation_date.timestamp().to_string()),
                    (
                        "ttl".to_string(),
                        format!("{}", resource_ttl.map(|ttl| ttl.as_secs()).unwrap_or(0)),
                    ),
                ]),
            )
            .map(|r| (r, RepositoryInfo { created: true }))
            .map_err(|e| ContainerRegistryError::CannotCreateRepository {
                registry_name: self.name.to_string(),
                repository_name: repository_name.to_string(),
                raw_error_message: e.to_string(),
            })
    }

    fn get_or_create_repository(
        &self,
        repository_name: &str,
        image_retention_time_in_seconds: u32,
        resource_ttl: Option<Duration>,
    ) -> Result<(Repository, RepositoryInfo), ContainerRegistryError> {
        // check if the repository already exists
        if let Ok(repository) = self.get_repository(repository_name) {
            return Ok((repository, RepositoryInfo { created: false }));
        }

        // create it if it doesn't exist
        match self.create_repository_without_exist_check(repository_name, image_retention_time_in_seconds, resource_ttl)
        {
            Ok((repository, info)) => Ok((repository, info)),
            Err(e) => Err(e),
        }
    }
}

impl ContainerRegistry for GoogleArtifactRegistry {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::GcpArtifactRegistry
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn long_id(&self) -> &Uuid {
        &self.long_id
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn registry_info(&self) -> &ContainerRegistryInfo {
        &self.registry_info
    }

    fn create_registry(&self) -> Result<(), ContainerRegistryError> {
        // Nothing to do, registry already here
        Ok(())
    }

    fn create_repository(
        &self,
        repository_name: &str,
        image_retention_time_in_seconds: u32,
        resource_ttl: Option<Duration>,
    ) -> Result<(Repository, RepositoryInfo), ContainerRegistryError> {
        self.get_or_create_repository(repository_name, image_retention_time_in_seconds, resource_ttl)
    }

    fn get_repository(&self, repository_name: &str) -> Result<Repository, ContainerRegistryError> {
        self.service
            .get_repository(self.project_id.as_str(), self.region.clone(), repository_name)
            .map_err(|e| ContainerRegistryError::CannotGetRepository {
                registry_name: self.name.to_string(),
                repository_name: repository_name.to_string(),
                raw_error_message: e.to_string(),
            })
    }

    fn delete_repository(&self, repository_name: &str) -> Result<(), ContainerRegistryError> {
        self.service
            .delete_repository(self.project_id.as_str(), self.region.clone(), repository_name)
            .map_err(|e| ContainerRegistryError::CannotDeleteRepository {
                registry_name: self.name.to_string(),
                repository_name: repository_name.to_string(),
                raw_error_message: e.to_string(),
            })
    }

    fn delete_image(&self, image: &Image) -> Result<(), ContainerRegistryError> {
        self.service
            .delete_docker_image(
                self.project_id.as_str(),
                self.region.clone(),
                image.repository_name.as_str(),
                image
                    .name
                    .strip_prefix(&format!("{}/{}/", &self.project_id, image.repository_name()))
                    .unwrap_or(&image.name),
            )
            .map_err(|e| ContainerRegistryError::CannotDeleteImage {
                registry_name: self.name.to_string(),
                repository_name: image.repository_name.to_string(),
                image_name: image
                    .name
                    .strip_prefix(&format!("{}/{}/", &self.project_id, image.repository_name()))
                    .unwrap_or(&image.name)
                    .to_string(),
                raw_error_message: e.to_string(),
            })
    }

    fn image_exists(&self, image: &Image) -> bool {
        self.service
            .get_docker_image(
                self.project_id.as_str(),
                self.region.clone(),
                image.repository_name.as_str(),
                image
                    .name
                    .strip_prefix(&format!("{}/{}/", &self.project_id, image.repository_name()))
                    .unwrap_or(&image.name),
                image.tag.as_str(),
            )
            .is_ok()
    }
}

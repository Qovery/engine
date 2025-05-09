use crate::environment::models::ToCloudProviderFormat;
use crate::environment::models::gcp::JsonCredentials;
use crate::environment::models::gcp::io::JsonCredentials as JsonCredentialsIo;
use crate::infrastructure::models::build_platform::Image;
use crate::infrastructure::models::cloud_provider::gcp::locations::GcpRegion;
use crate::infrastructure::models::container_registry::errors::ContainerRegistryError;
use crate::infrastructure::models::container_registry::{
    ContainerRegistryInfo, InteractWithRegistry, Kind, Repository, RepositoryInfo,
    take_last_x_chars_and_remove_leading_dash_char,
};
use crate::io_models::context::Context;
use crate::services::gcp::artifact_registry_service::ArtifactRegistryService;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use url::Url;
use uuid::Uuid;

use super::RegistryTags;

pub struct GoogleArtifactRegistry {
    context: Context,
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
        let registry_raw_url = format!("https://{}-docker.pkg.dev", region.to_cloud_provider_format());

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
        let project_name2 = project_id.to_string();
        const MAX_REGISTRY_NAME_LENGTH: usize = 53; // 63 (Artifact Registry limit) - 10 (prefix length)
        let registry_info = ContainerRegistryInfo {
            registry_name: name.to_string(),
            get_registry_endpoint: Box::new(move |_registry_url_prefix| registry.clone()),
            get_registry_url_prefix: Box::new(|_repository_name| None),
            get_registry_docker_json_config: Box::new(move |_docker_registry_info| None),
            insecure_registry: false,
            get_shared_image_name: Box::new(move |image_build_context| {
                let git_repo_truncated: String = take_last_x_chars_and_remove_leading_dash_char(
                    image_build_context.git_repo_url_sanitized.as_str(),
                    MAX_REGISTRY_NAME_LENGTH,
                );
                format!(
                    "{}/{}-{}/built-by-qovery",
                    &project_name,
                    image_build_context.cluster_id.short(),
                    git_repo_truncated
                )
            }),
            get_image_name: Box::new(move |img_name| {
                format!(
                    "{}/{}/{img_name}",
                    &project_name2,
                    match img_name.starts_with("qovery-") {
                        true => img_name.to_string(),
                        false => format!("qovery-{img_name}"), // repository name must start with a letter, then forcing `qovery-` prefix
                    }
                )
            }),
            get_shared_repository_name: Box::new(|image_build_context| {
                let git_repo_truncated: String = take_last_x_chars_and_remove_leading_dash_char(
                    image_build_context.git_repo_url_sanitized.as_str(),
                    MAX_REGISTRY_NAME_LENGTH,
                );
                format!("{}-{}", image_build_context.cluster_id.short(), git_repo_truncated)
            }),
            get_repository_name: Box::new(|repository_name| match repository_name.starts_with("qovery-") {
                true => repository_name.to_string(),
                false => format!("qovery-{repository_name}"), // repository name must start with a letter, then forcing `qovery-` prefix
            }),
        };

        Ok(Self {
            context,
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
        registry_tags: RegistryTags,
    ) -> Result<(Repository, RepositoryInfo), ContainerRegistryError> {
        let creation_date: DateTime<Utc> = Utc::now();
        let mut tags = vec![
            ("creation_date".to_string(), creation_date.timestamp().to_string()),
            (
                "ttl".to_string(),
                format!("{}", registry_tags.resource_ttl.map(|ttl| ttl.as_secs()).unwrap_or(0)),
            ),
        ];
        if let Some(environment_id) = registry_tags.environment_id {
            tags.push(("environment_id".to_string(), environment_id));
        }
        if let Some(project_id) = registry_tags.project_id {
            tags.push(("project_id".to_string(), project_id));
        }
        if let Some(cluster_id) = registry_tags.cluster_id {
            tags.push(("cluster_id".to_string(), cluster_id));
        }
        self.service
            .create_repository(&self.project_id, self.region.clone(), repository_name, HashMap::from_iter(tags))
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
        registry_tags: RegistryTags,
    ) -> Result<(Repository, RepositoryInfo), ContainerRegistryError> {
        // check if the repository already exists
        if let Ok(repository) = self.get_repository(repository_name) {
            return Ok((repository, RepositoryInfo { created: false }));
        }

        // create it if it doesn't exist
        match self.create_repository_without_exist_check(
            repository_name,
            image_retention_time_in_seconds,
            registry_tags,
        ) {
            Ok((repository, info)) => Ok((repository, info)),
            Err(e) => Err(e),
        }
    }
}

impl InteractWithRegistry for GoogleArtifactRegistry {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::GcpArtifactRegistry
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

    fn get_registry_endpoint(&self, registry_endpoint_prefix: Option<&str>) -> Url {
        self.registry_info().get_registry_endpoint(registry_endpoint_prefix)
    }

    fn create_repository(
        &self,
        _registry_name: Option<&str>,
        repository_name: &str,
        image_retention_time_in_seconds: u32,
        registry_tags: RegistryTags,
    ) -> Result<(Repository, RepositoryInfo), ContainerRegistryError> {
        self.get_or_create_repository(repository_name, image_retention_time_in_seconds, registry_tags)
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
                image.repository_name(),
                image
                    .name
                    .strip_prefix(&format!("{}/{}/", &self.project_id, image.repository_name()))
                    .unwrap_or(&image.name),
            )
            .map_err(|e| ContainerRegistryError::CannotDeleteImage {
                registry_name: self.name.to_string(),
                repository_name: image.repository_name().to_string(),
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
                image.repository_name(),
                image
                    .name
                    .strip_prefix(&format!("{}/{}/", &self.project_id, image.repository_name()))
                    .unwrap_or(&image.name),
                image.tag.as_str(),
            )
            .is_ok()
    }
}

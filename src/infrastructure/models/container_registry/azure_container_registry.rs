use crate::infrastructure::models::build_platform::Image;
use crate::infrastructure::models::cloud_provider::azure::locations::AzureLocation;
use crate::infrastructure::models::container_registry::errors::ContainerRegistryError;
use crate::infrastructure::models::container_registry::{
    ContainerRegistry, ContainerRegistryInfo, RegistryTags, Repository, RepositoryInfo,
    take_last_x_chars_and_remove_leading_dash_char,
};
use crate::io_models::context::Context;
use crate::services::azure::container_registry_service::{AzureContainerRegistryService, MAX_REGISTRY_NAME_LENGTH};
use azure_mgmt_containerregistry::models::{Sku, sku};
use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use url::Url;
use uuid::Uuid;

pub struct AzureContainerRegistry {
    context: Context,
    long_id: Uuid,
    name: String,
    subscription_id: String,
    resource_group_name: String,
    location: AzureLocation,
    registry_info: ContainerRegistryInfo,
    service: Arc<AzureContainerRegistryService>,
}

impl AzureContainerRegistry {
    pub fn new(
        context: Context,
        long_id: Uuid,
        name: &str,
        subscription_id: &str,
        resource_group_name: &str,
        client_id: &str,
        client_secret: &str,
        location: AzureLocation,
        service: Arc<AzureContainerRegistryService>,
    ) -> Result<Self, ContainerRegistryError> {
        let registry_raw_url = "https://azurecr.io";

        let mut registry = Url::parse(registry_raw_url).map_err(|_e| ContainerRegistryError::InvalidRegistryUrl {
            registry_url: registry_raw_url.to_string(),
        })?;
        let _ = registry.set_username(client_id);
        let _ = registry.set_password(Some(client_secret));

        let repository_base_name = name.to_string();
        let repository_base_name2 = name.to_string();

        let registry_info = ContainerRegistryInfo {
            endpoint: registry,
            registry_name: name.to_string(),
            registry_docker_json_config: None,
            insecure_registry: false,
            get_shared_image_name: Box::new(move |image_build_context| {
                let git_repo_truncated: String = take_last_x_chars_and_remove_leading_dash_char(
                    image_build_context.git_repo_url_sanitized.as_str(),
                    MAX_REGISTRY_NAME_LENGTH,
                );
                format!(
                    "{}/{}-{}/built-by-qovery",
                    &repository_base_name,
                    image_build_context.cluster_id.short(),
                    git_repo_truncated
                )
            }),
            get_image_name: Box::new(move |img_name| {
                format!(
                    "{}/{}/{img_name}",
                    &repository_base_name2.to_string(),
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
            subscription_id: subscription_id.to_string(),
            resource_group_name: resource_group_name.to_string(),
            location,
            registry_info,
            service,
        })
    }

    fn create_repository_without_exist_check(
        &self,
        repository_name: &str,
        image_retention_duration: Duration,
        registry_tags: RegistryTags,
    ) -> Result<(Repository, RepositoryInfo), ContainerRegistryError> {
        let creation_date: DateTime<Utc> = Utc::now();
        self.service
            .create_registry(
                self.subscription_id.as_str(),
                self.resource_group_name.as_str(),
                self.location.clone(),
                repository_name,
                Sku::new(sku::Name::Basic),
                Some(image_retention_duration),
                Some(HashMap::from([
                    ("creation_date".to_string(), creation_date.timestamp().to_string()),
                    (
                        "ttl".to_string(),
                        format!("{}", registry_tags.resource_ttl.map(|ttl| ttl.as_secs()).unwrap_or(0)),
                    ),
                    ("environment_id".to_string(), registry_tags.environment_id),
                    ("project_id".to_string(), registry_tags.project_id),
                ])),
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
        image_retention_duration: Duration,
        registry_tags: RegistryTags,
    ) -> Result<(Repository, RepositoryInfo), ContainerRegistryError> {
        // check if the repository already exists
        if let Ok(repository) = self.get_repository(repository_name) {
            return Ok((repository, RepositoryInfo { created: false }));
        }

        // create it if it doesn't exist
        match self.create_repository_without_exist_check(repository_name, image_retention_duration, registry_tags) {
            Ok((repository, info)) => Ok((repository, info)),
            Err(e) => Err(e),
        }
    }
}

impl ContainerRegistry for AzureContainerRegistry {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> super::Kind {
        super::Kind::AzureContainerRegistry
    }

    fn long_id(&self) -> &Uuid {
        &self.long_id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn registry_info(&self) -> &ContainerRegistryInfo {
        &self.registry_info
    }

    fn create_repository(
        &self,
        repository_name: &str,
        image_retention_time_in_seconds: u32,
        registry_tags: RegistryTags,
    ) -> Result<(Repository, RepositoryInfo), ContainerRegistryError> {
        self.get_or_create_repository(
            repository_name,
            Duration::seconds(image_retention_time_in_seconds as i64),
            registry_tags,
        )
    }

    fn get_repository(&self, repository_name: &str) -> Result<Repository, ContainerRegistryError> {
        self.service
            .get_registry(
                self.subscription_id.as_str(),
                self.resource_group_name.as_str(),
                repository_name,
            )
            .map_err(|e| ContainerRegistryError::CannotGetRepository {
                registry_name: self.name.to_string(),
                repository_name: repository_name.to_string(),
                raw_error_message: e.to_string(),
            })
    }

    fn delete_repository(&self, repository_name: &str) -> Result<(), ContainerRegistryError> {
        self.service
            .delete_registry(
                self.subscription_id.as_str(),
                self.resource_group_name.as_str(),
                repository_name,
            )
            .map_err(|e| ContainerRegistryError::CannotGetRepository {
                registry_name: self.name.to_string(),
                repository_name: repository_name.to_string(),
                raw_error_message: e.to_string(),
            })
    }

    fn delete_image(&self, _image_name: &Image) -> Result<(), ContainerRegistryError> {
        todo!()
    }

    fn image_exists(&self, _image: &Image) -> bool {
        todo!()
    }
}

use crate::infrastructure::models::build_platform::Image;
use crate::infrastructure::models::cloud_provider::azure::locations::AzureLocation;
use crate::infrastructure::models::container_registry::errors::ContainerRegistryError;
use crate::infrastructure::models::container_registry::{
    ContainerRegistryInfo, DockerRegistryInfo, InteractWithRegistry, RegistryTags, Repository, RepositoryInfo,
    take_last_x_chars_and_remove_leading_dash_char,
};
use crate::io_models::context::Context;
use crate::services::azure::container_registry_service::{AzureContainerRegistryService, MAX_REGISTRY_NAME_LENGTH};
use azure_mgmt_containerregistry::models::{Sku, sku};
use base64::Engine;
use base64::engine::general_purpose;
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

        let client_id_clone = client_id.to_string(); // for closure
        let client_secret_clone = client_secret.to_string(); // for closure
        let registry_info = ContainerRegistryInfo {
            registry_endpoint: registry.clone(),
            registry_name: name.to_string(),
            get_registry_docker_json_config: Box::new(move |docker_registry_info| {
                Some(Self::get_docker_json_config_raw(
                    &client_id_clone,
                    &client_secret_clone,
                    docker_registry_info,
                ))
            }),
            insecure_registry: false,
            get_registry_url_prefix: Box::new(|cluster_id| {
                Some(
                    AzureContainerRegistryService::try_get_sanitized_registry_name(
                        format!("qovery-{}", cluster_id.short()).as_str(),
                    )
                    .unwrap_or_default(),
                )
            }),
            get_shared_image_name: Box::new(move |image_build_context| {
                let git_repo_truncated: String = take_last_x_chars_and_remove_leading_dash_char(
                    image_build_context.git_repo_url_sanitized.as_str(),
                    MAX_REGISTRY_NAME_LENGTH,
                );
                format!(
                    "{}-{}/built-by-qovery",
                    image_build_context.cluster_id.short(),
                    git_repo_truncated
                )
            }),
            get_image_name: Box::new(move |img_name| {
                format!(
                    "{}/{img_name}",
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
        resource_group_name: Option<&str>,
        repository_name: &str,
        image_retention_duration: Duration,
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
            .create_registry(
                self.subscription_id.as_str(),
                resource_group_name.unwrap_or(self.resource_group_name.as_str()),
                self.location.clone(),
                repository_name,
                Sku::new(sku::Name::Basic),
                Some(image_retention_duration),
                Some(HashMap::from_iter(tags)),
            )
            .map(|r| (r, RepositoryInfo { created: true }))
            .map_err(|e| ContainerRegistryError::CannotCreateRepository {
                registry_name: self.name.to_string(),
                repository_name: repository_name.to_string(),
                raw_error_message: e.to_string(),
            })
    }

    pub fn create_repository_in_resource_group(
        &self,
        resource_group_name: Option<&str>,
        repository_name: &str,
        image_retention_time_in_seconds: u32,
        registry_tags: RegistryTags,
    ) -> Result<(Repository, RepositoryInfo), ContainerRegistryError> {
        self.get_or_create_repository(
            resource_group_name,
            repository_name,
            Duration::seconds(image_retention_time_in_seconds as i64),
            registry_tags,
        )
    }

    fn get_or_create_repository(
        &self,
        resource_group_name: Option<&str>,
        repository_name: &str,
        image_retention_duration: Duration,
        registry_tags: RegistryTags,
    ) -> Result<(Repository, RepositoryInfo), ContainerRegistryError> {
        // check if the repository already exists
        if let Ok(repository) = self.service.get_registry(
            self.subscription_id.as_str(),
            resource_group_name.unwrap_or(self.resource_group_name.as_str()),
            repository_name,
        ) {
            return Ok((repository, RepositoryInfo { created: false }));
        }

        // create it if it doesn't exist
        match self.create_repository_without_exist_check(
            resource_group_name,
            repository_name,
            image_retention_duration,
            registry_tags,
        ) {
            Ok((repository, info)) => Ok((repository, info)),
            Err(e) => Err(e),
        }
    }

    fn get_docker_json_config_raw(login: &str, secret_token: &str, docker_registry_info: DockerRegistryInfo) -> String {
        general_purpose::STANDARD.encode(
            format!(
                r#"{{"auths":{{"{}.azurecr.io":{{"auth":"{}"}}}}}}"#,
                match docker_registry_info.registry_name {
                    Some(registry_name) =>
                        AzureContainerRegistryService::try_get_sanitized_registry_name(registry_name.as_str())
                            .unwrap_or_default(),
                    None => {
                        match docker_registry_info.image_name {
                            Some(image_name) => image_name
                                .split_once('/')
                                .map(|(repository_name, _)| {
                                    AzureContainerRegistryService::try_get_sanitized_registry_name(repository_name)
                                        .unwrap_or_default()
                                })
                                .unwrap_or(image_name.to_string())
                                .to_string(),
                            None => "".to_string(), // Shouldn't happen, if so, it will fail
                        }
                    }
                },
                general_purpose::STANDARD.encode(format!("{login}:{secret_token}").as_bytes())
            )
            .as_bytes(),
        )
    }
}

impl InteractWithRegistry for AzureContainerRegistry {
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
        registry_name: Option<&str>,
        _repository_name: &str,
        image_retention_time_in_seconds: u32,
        registry_tags: RegistryTags,
    ) -> Result<(Repository, RepositoryInfo), ContainerRegistryError> {
        // For Azure, we create the repository with the registry name
        if registry_name.is_none() {
            return Err(ContainerRegistryError::CannotCreateRepository {
                registry_name: self.name.to_string(),
                repository_name: _repository_name.to_string(),
                raw_error_message: "Registry name is required for Azure".to_string(),
            });
        }

        self.get_or_create_repository(
            Some(self.resource_group_name.as_str()),
            registry_name.unwrap_or_default(),
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

    fn delete_image(&self, image: &Image) -> Result<(), ContainerRegistryError> {
        self.service
            .delete_docker_image(
                self.subscription_id.as_str(),
                self.resource_group_name.as_str(),
                image.repository_name.as_str(),
                image.name.as_str(),
                image.tag.as_str(),
            )
            .map_err(|e| ContainerRegistryError::CannotGetRepository {
                registry_name: self.name.to_string(),
                repository_name: image.repository_name.to_string(),
                raw_error_message: e.to_string(),
            })
    }

    fn image_exists(&self, image: &Image) -> bool {
        self.service
            .get_docker_image(
                self.subscription_id.as_str(),
                self.resource_group_name.as_str(),
                image.registry_name.as_str(),
                image.name.as_str(),
                image.tag.as_str(),
            )
            .is_ok()
    }
}

use crate::infrastructure::models::build_platform::Image;
use crate::infrastructure::models::cloud_provider::azure::locations::AzureLocation;
use crate::infrastructure::models::container_registry::errors::ContainerRegistryError;
use crate::infrastructure::models::container_registry::{
    ContainerRegistryInfo, InteractWithRegistry, RegistryTags, Repository, RepositoryInfo,
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

        let registry_raw_cluster_url = format!("https://qovery{}.azurecr.io", context.cluster_short_id());
        let mut registry_cluster =
            Url::parse(registry_raw_cluster_url.as_str()).map_err(|_e| ContainerRegistryError::InvalidRegistryUrl {
                registry_url: registry_raw_cluster_url.to_string(),
            })?;
        let _ = registry_cluster.set_username(client_id);
        let _ = registry_cluster.set_password(Some(client_secret));

        let registry_info = ContainerRegistryInfo {
            registry_name: name.to_string(),
            get_registry_endpoint: Box::new(move |registry_endpoint_prefix| {
                let sanitized_name = registry_endpoint_prefix
                    .map(|e| AzureContainerRegistryService::try_get_sanitized_registry_name(e).unwrap_or_default());

                let mut registry_url = registry.clone();
                let _ =
                    registry_url.set_host(Some(format!("{}.azurecr.io", sanitized_name.unwrap_or_default()).as_str()));

                registry_url
            }),
            get_registry_docker_json_config: Box::new(move |_docker_registry_info| None),
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

        let cr = Self {
            context,
            long_id,
            name: name.to_string(),
            subscription_id: subscription_id.to_string(),
            resource_group_name: resource_group_name.to_string(),
            location,
            registry_info,
            service,
        };

        // Login to the registry if cluster has already been deployed
        if !cr.context.is_first_cluster_deployment() {
            // login to cluster registry
            cr.context
                .docker
                .login_with_retry(&registry_cluster)
                .map_err(|_err| ContainerRegistryError::InvalidCredentials)?;
        }

        Ok(cr)
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
        resource_group_name: &str,
        repository_name: &str,
        image_retention_time_in_seconds: u32,
        registry_tags: RegistryTags,
    ) -> Result<(Repository, RepositoryInfo), ContainerRegistryError> {
        self.get_or_create_repository(
            Some(resource_group_name),
            repository_name,
            Duration::seconds(image_retention_time_in_seconds as i64),
            registry_tags,
        )
    }

    pub fn delete_repository_in_resource_group(
        &self,
        resource_group_name: &str,
        repository_name: &str,
    ) -> Result<(), ContainerRegistryError> {
        self.service
            .delete_registry(self.subscription_id.as_str(), resource_group_name, repository_name)
            .map_err(|e| ContainerRegistryError::CannotDeleteRepository {
                registry_name: self.name.clone(),
                repository_name: repository_name.to_string(),
                raw_error_message: e.to_string(),
            })
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

    /// This function extracts the registry name from a full URL or hostname.
    /// Azure registries follow the format `registry_name.azurecr.io`.
    pub fn get_registry_name_from_url(registry_url: &Url) -> Option<String> {
        let suffix = ".azurecr.io";
        registry_url
            .host_str()
            .filter(|host| host.ends_with(suffix))
            .and_then(|host| {
                let end = host.len() - suffix.len();
                if end > 0 { Some(host[..end].to_string()) } else { None }
            })
    }

    pub fn allow_cluster_to_pull_from_registry(&self, cluster_name: &str) -> Result<(), ContainerRegistryError> {
        self.service
            .allow_cluster_to_pull_from_registry(self.resource_group_name.as_str(), &self.name, cluster_name) // there is only one registry per cluster named after cluster ID
            .map_err(|e| ContainerRegistryError::CannotLinkRegistryToCluster {
                registry_name: self.name.clone(),
                cluster_id: cluster_name.to_string(),
                raw_error_message: e.to_string(),
            })
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

    fn get_registry_endpoint(&self, registry_endpoint_prefix: Option<&str>) -> Url {
        let sanitized_name = registry_endpoint_prefix
            .map(|e| AzureContainerRegistryService::try_get_sanitized_registry_name(e).unwrap_or_default());
        self.registry_info().get_registry_endpoint(sanitized_name.as_deref())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_get_registry_name_from_url() {
        // setup:
        struct TestCase<'a> {
            url: Url,
            expected: Option<&'a str>,
        }

        let test_cases = vec![
            TestCase {
                url: Url::from_str("https://myregistry.azurecr.io").expect("Valid URL"),
                expected: Some("myregistry"),
            },
            TestCase {
                url: Url::from_str("https://another.registry.azurecr.io").expect("Valid URL"),
                expected: Some("another.registry"),
            },
            TestCase {
                url: Url::from_str("https://myregistry.azurecr.io/some/path").expect("Valid URL"),
                expected: Some("myregistry"),
            },
        ];

        for tc in test_cases {
            // execute and validate:
            assert_eq!(
                AzureContainerRegistry::get_registry_name_from_url(&tc.url),
                tc.expected.map(|s| s.to_string())
            );
        }
    }
}

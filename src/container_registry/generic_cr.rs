use std::time::Duration;

use crate::build_platform::Image;
use crate::container_registry::errors::ContainerRegistryError;
use crate::container_registry::{ContainerRegistry, ContainerRegistryInfo, Kind, Repository, RepositoryInfo};

use crate::io_models::context::Context;

use crate::cmd::docker::ContainerImage;
use crate::cmd::skopeo::Skopeo;
use url::Url;
use uuid::Uuid;

pub struct GenericCr {
    context: Context,
    long_id: Uuid,
    name: String,
    url: Url,
    skip_tls_verification: bool,
    _repository_name: String,
    skopeo: Skopeo,
    cr_info: ContainerRegistryInfo,
}

impl GenericCr {
    pub fn new(
        context: Context,
        long_id: Uuid,
        name: &str,
        url: Url,
        skip_tls_verification: bool,
        repository_name: String,
        credentials: Option<(String, String)>,
    ) -> Result<Self, ContainerRegistryError> {
        let skopeo = Skopeo::new(credentials).map_err(|err| ContainerRegistryError::CannotInstantiateClient {
            raw_error_message: err.to_string(),
        })?;

        let repository = repository_name.clone();
        let container_registry_info = ContainerRegistryInfo {
            endpoint: url.clone(),
            registry_name: name.to_string(),
            registry_docker_json_config: None,
            get_image_name: Box::new(move |name| format!("{}/{}", repository, name)),
            get_repository_name: Box::new(|name| name.to_string()),
        };

        let cr = Self {
            context,
            long_id,
            name: name.to_string(),
            url,
            skip_tls_verification,
            _repository_name: repository_name,
            skopeo,
            cr_info: container_registry_info,
        };

        //cr.is_credentials_valid()?;
        Ok(cr)
    }
}

impl ContainerRegistry for GenericCr {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::LocalRegistry
    }

    fn id(&self) -> &str {
        ""
    }

    fn long_id(&self) -> &Uuid {
        &self.long_id
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn registry_info(&self) -> &ContainerRegistryInfo {
        &self.cr_info
    }

    fn create_registry(&self) -> Result<(), ContainerRegistryError> {
        // Nothing to do, local registry create automatically new repositories
        Ok(())
    }

    fn create_repository(
        &self,
        name: &str,
        _image_retention_time_in_seconds: u32,
        _resource_ttl: Option<Duration>,
    ) -> Result<(Repository, RepositoryInfo), ContainerRegistryError> {
        // Nothing to do, local registry create automatically new repositories
        Ok((
            Repository {
                registry_id: name.to_string(),
                name: name.to_string(),
                uri: Some(self.url.join(name).map(|u| u.to_string()).unwrap_or_default()),
                ttl: None,
                labels: None,
            },
            RepositoryInfo { created: false },
        ))
    }

    fn get_repository(&self, repository_name: &str) -> Result<Repository, ContainerRegistryError> {
        Ok(Repository {
            registry_id: repository_name.to_string(),
            name: repository_name.to_string(),
            uri: Some(
                self.url
                    .join(repository_name)
                    .map(|u| u.to_string())
                    .unwrap_or_default(),
            ),
            ttl: None,
            labels: None,
        })
    }

    fn delete_repository(&self, repository_name: &str) -> Result<(), ContainerRegistryError> {
        let container =
            ContainerImage::new(self.cr_info.endpoint.clone(), repository_name.to_string(), vec!["".to_string()]);
        let tags = self
            .skopeo
            .list_tags(&container, !self.skip_tls_verification)
            .map_err(|err| ContainerRegistryError::CannotDeleteRepository {
                registry_name: self.name.clone(),
                repository_name: repository_name.to_string(),
                raw_error_message: err.to_string(),
            })?;

        for tag in tags {
            let container = ContainerImage::new(self.cr_info.endpoint.clone(), repository_name.to_string(), vec![tag]);
            self.skopeo
                .delete_image(&container, !self.skip_tls_verification)
                .map_err(|err| ContainerRegistryError::CannotDeleteRepository {
                    registry_name: self.name.clone(),
                    repository_name: repository_name.to_string(),
                    raw_error_message: err.to_string(),
                })?;
        }

        Ok(())
    }

    fn delete_image(&self, image: &Image) -> Result<(), ContainerRegistryError> {
        let container = ContainerImage::new(self.cr_info.endpoint.clone(), image.name.clone(), vec![image.tag.clone()]);
        self.skopeo
            .delete_image(&container, !self.skip_tls_verification)
            .map_err(|err| ContainerRegistryError::CannotDeleteImage {
                registry_name: self.name.clone(),
                repository_name: image.repository_name().to_string(),
                image_name: image.name().to_string(),
                raw_error_message: err.to_string(),
            })?;

        Ok(())
    }

    fn image_exists(&self, image: &Image) -> bool {
        let container = ContainerImage::new(self.cr_info.endpoint.clone(), image.name.clone(), vec![image.tag.clone()]);
        let Ok(tags) = self.skopeo.list_tags(&container, !self.skip_tls_verification) else {
            return false;
        };

        tags.contains(&image.tag)
    }
}

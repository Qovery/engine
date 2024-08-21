use crate::build_platform::Image;
use crate::container_registry::errors::ContainerRegistryError;
use crate::container_registry::{ContainerRegistry, ContainerRegistryInfo, Kind, Repository, RepositoryInfo};
use crate::io_models::context::Context;
use itertools::Itertools;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION};
use serde_derive::Deserialize;
use std::time::Duration;

use super::RegistryTags;
use crate::cmd::docker::ContainerImage;
use crate::container_registry::generic_cr::GenericCr;
use url::Url;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub enum RegistryType {
    User(String),
    Organization(String),
}

impl RegistryType {
    fn repository_prefix(&self) -> &str {
        match self {
            RegistryType::User(user) => user.as_str(),
            RegistryType::Organization(orga) => orga.as_str(),
        }
    }
}

pub struct GithubCr {
    generic_cr: GenericCr,
    http_client: reqwest::blocking::Client,
    registry_type: RegistryType,
}

impl GithubCr {
    pub fn new(
        context: Context,
        long_id: Uuid,
        name: &str,
        url: Url,
        repository_type: RegistryType,
        token: String,
    ) -> Result<Self, ContainerRegistryError> {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/vnd.github+json"));
        headers.insert("X-GitHub-Api-Version", HeaderValue::from_static("2022-11-28"));
        let mut auth_header = HeaderValue::from_str(&format!("Bearer {}", &token)).map_err(|e| {
            ContainerRegistryError::CannotInstantiateClient {
                raw_error_message: format!("Cannot create auth header: {}", e),
            }
        })?;
        auth_header.set_sensitive(true);
        headers.insert(AUTHORIZATION, auth_header);
        let http_client = reqwest::blocking::Client::builder()
            .default_headers(headers)
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            // All api call must have an user agent set
            // https://docs.github.com/en/rest/using-the-rest-api/getting-started-with-the-rest-api?apiVersion=2022-11-28#user-agent
            .user_agent("qovery-engine")
            //.proxy(reqwest::Proxy::all("http://localhost:8080").unwrap())
            .build()
            .map_err(|e| ContainerRegistryError::CannotInstantiateClient {
                raw_error_message: format!("Cannot create http client: {}", e),
            })?;

        let generic_cr = GenericCr::new(
            context,
            long_id,
            name,
            url,
            false,
            repository_type.repository_prefix().to_string(),
            Some(("nologin".to_string(), token)),
            true,
        )?;

        let cr = Self {
            generic_cr,
            http_client,
            registry_type: repository_type,
        };

        Ok(cr)
    }
}

impl ContainerRegistry for GithubCr {
    fn context(&self) -> &Context {
        self.generic_cr.context()
    }

    fn kind(&self) -> Kind {
        Kind::GithubCr
    }

    fn long_id(&self) -> &Uuid {
        self.generic_cr.long_id()
    }

    fn name(&self) -> &str {
        self.generic_cr.name()
    }

    fn registry_info(&self) -> &ContainerRegistryInfo {
        self.generic_cr.registry_info()
    }

    fn create_registry(&self) -> Result<(), ContainerRegistryError> {
        self.generic_cr.create_registry()
    }

    fn create_repository(
        &self,
        name: &str,
        image_retention_time_in_seconds: u32,
        registry_tags: RegistryTags,
    ) -> Result<(Repository, RepositoryInfo), ContainerRegistryError> {
        self.generic_cr
            .create_repository(name, image_retention_time_in_seconds, registry_tags)
    }

    fn get_repository(&self, repository_name: &str) -> Result<Repository, ContainerRegistryError> {
        self.generic_cr.get_repository(repository_name)
    }

    fn delete_repository(&self, repository_name: &str) -> Result<(), ContainerRegistryError> {
        // Github api does not want the user prefix. i.e: qovery/engine -> engine
        let repository_name = if let Some((_, repo)) = repository_name.split_once('/') {
            repo
        } else {
            repository_name
        };

        // https://api.github.com/user/packages/container/
        // https://api.github.com/orgs/ORG/packages/container/PACKAGE_NAME
        let api_url = match &self.registry_type {
            RegistryType::User(_) => format!("https://api.github.com/user/packages/container/{}", repository_name),
            RegistryType::Organization(org) => {
                format!("https://api.github.com/orgs/{}/packages/container/{}", org, repository_name)
            }
        };
        match self
            .http_client
            .delete(api_url)
            .send()
            .map(|res| res.error_for_status())
        {
            Ok(_) => Ok(()),
            Err(err) if matches!(err.status(), Some(reqwest::StatusCode::NOT_FOUND)) => Ok(()),
            Err(err) => Err(ContainerRegistryError::CannotDeleteRepository {
                registry_name: self.name().to_string(),
                repository_name: repository_name.to_string(),
                raw_error_message: err.to_string(),
            }),
        }
    }

    fn delete_image(&self, image: &Image) -> Result<(), ContainerRegistryError> {
        let to_error = |raw_error_message: String| ContainerRegistryError::CannotDeleteImage {
            registry_name: self.name().to_string(),
            repository_name: image.repository_name().to_string(),
            image_name: image.name.to_string(),
            raw_error_message,
        };

        #[derive(Default, Deserialize)]
        struct ImageVersion {
            id: u64,
            name: String, // the digest, start with sha256:
            metadata: ImageMetadata,
        }
        #[derive(Default, Deserialize)]
        struct ImageMetadata {
            container: ImageContainer,
        }
        #[derive(Default, Deserialize)]
        struct ImageContainer {
            tags: Vec<String>,
        }

        fn list_versions(this: &GithubCr, repository_name: &str) -> reqwest::Result<Vec<ImageVersion>> {
            let api_url = match &this.registry_type {
                RegistryType::User(_) => {
                    format!("https://api.github.com/user/packages/container/{}/versions", repository_name)
                }
                RegistryType::Organization(org) => format!(
                    "https://api.github.com/orgs/{}/packages/container/{}/versions",
                    org, repository_name
                ),
            };

            match this
                .http_client
                .get(api_url)
                .send()
                .and_then(|res| res.error_for_status())
            {
                Ok(res) => Ok(res.json().unwrap_or_default()),
                Err(err) if matches!(err.status(), Some(reqwest::StatusCode::NOT_FOUND)) => Ok(vec![]),
                Err(err) => Err(err),
            }
        }

        fn delete_version(this: &GithubCr, repository_name: &str, version_id: u64) -> reqwest::Result<()> {
            // https://docs.github.com/en/rest/packages/packages?apiVersion=2022-11-28#delete-package-version-for-an-organization
            // https://docs.github.com/en/rest/packages/packages?apiVersion=2022-11-28#delete-a-package-version-for-the-authenticated-user
            let api_url = match &this.registry_type {
                RegistryType::User(_) => format!(
                    "https://api.github.com/user/packages/container/{}/versions/{}",
                    repository_name, version_id
                ),
                RegistryType::Organization(org) => format!(
                    "https://api.github.com/orgs/{}/packages/container/{}/versions/{}",
                    org, repository_name, version_id
                ),
            };

            match this
                .http_client
                .delete(api_url)
                .send()
                .and_then(|res| res.error_for_status())
            {
                Ok(_res) => Ok(()),
                Err(err) if matches!(err.status(), Some(reqwest::StatusCode::NOT_FOUND)) => Ok(()),
                Err(err) => Err(err),
            }
        }

        // list all versions/digest for this image to get the version id
        // Github has its own version/id system for layers, they don't use the sha256 digest for that.
        let versions = list_versions(self, image.name_without_repository()).map_err(|e| to_error(e.to_string()))?;

        // Github forbid to delete the last tag of an image, in this case you must delete the repository itself.
        let tags = versions
            .iter()
            .flat_map(|v| v.metadata.container.tags.as_slice())
            .collect_vec();
        if tags.len() == 1 && tags[0] == &image.tag {
            return self.delete_repository(&image.name);
        }

        // list all the digest belonging to this image
        // If you delete the tag, GithubCr does not also delete the other layers of the image (i.e: multi-arch images)
        // They stay there forever, so we need to delete them manually
        let container = ContainerImage::new(
            self.generic_cr.registry_info().endpoint.clone(),
            image.name.clone(),
            vec![image.tag.clone()],
        );
        let image_digests = self
            .generic_cr
            .skopeo()
            .list_digests(&container, true)
            .map_err(|e| to_error(e.to_string()))?;

        for digest in versions.iter().filter(|v| image_digests.contains(&v.name)) {
            let _ = delete_version(self, image.name_without_repository(), digest.id);
        }

        Ok(())
    }

    fn image_exists(&self, image: &Image) -> bool {
        self.generic_cr.image_exists(image)
    }
}

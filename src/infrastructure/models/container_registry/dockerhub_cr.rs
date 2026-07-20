use super::RegistryTags;
use crate::infrastructure::models::build_platform::Image;
use crate::infrastructure::models::container_registry::errors::ContainerRegistryError;
use crate::infrastructure::models::container_registry::generic_cr::GenericCr;
use crate::infrastructure::models::container_registry::{
    ContainerRegistryInfo, InteractWithRegistry, Kind, Repository, RepositoryInfo,
};
use crate::io_models::context::Context;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_derive::{Deserialize, Serialize};
use std::sync::LazyLock;
use std::time::Duration;
use url::Url;
use uuid::Uuid;

// Docker Hub management API. This is distinct from the OCI registry endpoint (index.docker.io)
// which is used to push/pull images. Repository/tag deletion goes through this API.
const DOCKER_HUB_API_URL: &str = "https://hub.docker.com/v2";

// Canonical Docker Hub OCI registry endpoint. Docker Hub can be addressed through several
// hostnames (docker.io, registry-1.docker.io, index.docker.io), but `docker login`/`docker push`
// only share credentials when they resolve to the canonical index `https://index.docker.io/v1/`.
// Logging in against any other host (e.g. registry-1.docker.io) stores the credentials under a key
// that the push does not look up, resulting in `insufficient_scope: authorization failed`.
static DOCKER_HUB_REGISTRY_ENDPOINT: LazyLock<Url> =
    LazyLock::new(|| Url::parse("https://docker.io/v1/").expect("hardcoded Docker Hub registry URL is invalid"));

fn is_docker_hub_host(host: &str) -> bool {
    matches!(host, "docker.io" | "index.docker.io" | "registry-1.docker.io")
}

pub struct DockerHubCr {
    generic_cr: GenericCr,
    http_client: reqwest::blocking::Client,
    // The Docker Hub namespace (user or organization) images are pushed under.
    // i.e: docker.io/<namespace>/<image>
    namespace: String,
}

#[derive(Serialize)]
struct LoginRequest<'a> {
    username: &'a str,
    password: &'a str,
}

#[derive(Default, Deserialize)]
struct LoginResponse {
    token: String,
}

#[derive(Serialize)]
struct CreateRepositoryRequest<'a> {
    namespace: &'a str,
    name: &'a str,
    is_private: bool,
    description: &'a str,
}

#[derive(Default, Deserialize)]
struct RepositoryResponse {
    name: String,
}

impl DockerHubCr {
    pub fn new(
        context: Context,
        long_id: Uuid,
        name: &str,
        url: Url,
        username: String,
        token: String,
    ) -> Result<Self, ContainerRegistryError> {
        // The Docker Hub management API does not accept the docker password/PAT directly.
        // We must first exchange the credentials for a JWT token.
        let login_client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .user_agent("qovery-engine")
            .build()
            .map_err(|e| ContainerRegistryError::CannotInstantiateClient {
                raw_error_message: format!("Cannot create http client: {e}"),
            })?;

        let jwt_token: String = login_client
            .post(format!("{DOCKER_HUB_API_URL}/users/login"))
            .json(&LoginRequest {
                username: &username,
                password: &token,
            })
            .send()
            .and_then(|res| res.error_for_status())
            .and_then(|res| res.json::<LoginResponse>())
            .map(|res| res.token)
            .map_err(|_err| ContainerRegistryError::InvalidCredentials)?;

        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let mut auth_header = HeaderValue::from_str(&format!("JWT {jwt_token}")).map_err(|e| {
            ContainerRegistryError::CannotInstantiateClient {
                raw_error_message: format!("Cannot create auth header: {e}"),
            }
        })?;
        auth_header.set_sensitive(true);
        headers.insert(AUTHORIZATION, auth_header);
        let http_client = reqwest::blocking::Client::builder()
            .default_headers(headers)
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .user_agent("qovery-engine")
            //.proxy(reqwest::Proxy::all("http://localhost:8080").unwrap())
            .build()
            .map_err(|e| ContainerRegistryError::CannotInstantiateClient {
                raw_error_message: format!("Cannot create http client: {e}"),
            })?;

        // Normalize any Docker Hub hostname to the canonical registry index so that the credentials
        // stored by `docker login` are found again when pushing/pulling. Keep the caller provided URL
        // otherwise (should not happen for Docker Hub, but stays safe).
        let registry_url = match url.host_str() {
            Some(host) if is_docker_hub_host(host) => DOCKER_HUB_REGISTRY_ENDPOINT.clone(),
            _ => url,
        };

        // GenericCr handles the docker login/skopeo based operations (push/pull, image_exists, ...)
        // Images live under docker.io/<namespace>/<image>, so the namespace is used as repository prefix.
        let generic_cr = GenericCr::new(
            context,
            long_id,
            name,
            registry_url,
            false,
            username.clone(),
            Some((username.clone(), token)),
            true,
        )?;

        let cr = Self {
            generic_cr,
            http_client,
            namespace: username,
        };

        Ok(cr)
    }

    // Docker Hub repository name expected by the management API does not contain the namespace prefix.
    // i.e: qovery/engine -> engine
    fn repository_name_without_namespace<'a>(&self, repository_name: &'a str) -> &'a str {
        match repository_name.split_once('/') {
            Some((_, repo)) => repo,
            None => repository_name,
        }
    }

    fn to_repository(&self, repository_name: &str) -> Repository {
        // Docker Hub image uri: docker.io/<namespace>/<repository>
        Repository {
            registry_id: format!("{}/{repository_name}", self.namespace),
            name: repository_name.to_string(),
            uri: Some(format!("docker.io/{}/{repository_name}", self.namespace)),
            ttl: None,
            labels: None,
        }
    }
}

impl InteractWithRegistry for DockerHubCr {
    fn context(&self) -> &Context {
        self.generic_cr.context()
    }

    fn kind(&self) -> Kind {
        Kind::DockerHub
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

    fn get_registry_endpoint(&self, registry_endpoint_prefix: Option<&str>) -> Url {
        self.registry_info().get_registry_endpoint(registry_endpoint_prefix)
    }

    fn create_repository(
        &self,
        _registry_name: Option<&str>,
        name: &str,
        _image_retention_time_in_seconds: u32,
        _registry_tags: RegistryTags,
    ) -> Result<(Repository, RepositoryInfo), ContainerRegistryError> {
        let repository_name = self.repository_name_without_namespace(name);

        // If the repository already exists, return it directly instead of trying to create it again.
        if let Ok(repository) = self.get_repository(repository_name) {
            return Ok((repository, RepositoryInfo { created: false }));
        }

        // https://docs.docker.com/reference/api/hub/latest/#tag/repositories/operation/CreateRepository
        let api_url = format!("{DOCKER_HUB_API_URL}/repositories/");
        match self
            .http_client
            .post(api_url)
            .json(&CreateRepositoryRequest {
                namespace: &self.namespace,
                name: repository_name,
                is_private: true,
                description: "Managed by Qovery",
            })
            .send()
            .and_then(|res| res.error_for_status())
        {
            Ok(_) => Ok((self.to_repository(repository_name), RepositoryInfo { created: true })),
            Err(err) => Err(ContainerRegistryError::CannotCreateRepository {
                registry_name: self.name().to_string(),
                repository_name: repository_name.to_string(),
                raw_error_message: err.to_string(),
            }),
        }
    }

    fn get_repository(&self, repository_name: &str) -> Result<Repository, ContainerRegistryError> {
        let repository_name = self.repository_name_without_namespace(repository_name);

        // https://docs.docker.com/reference/api/hub/latest/#tag/repositories/operation/GetNamespaceRepository
        let api_url = format!("{DOCKER_HUB_API_URL}/repositories/{}/{repository_name}", self.namespace);
        match self
            .http_client
            .get(api_url)
            .send()
            .and_then(|res| res.error_for_status())
            .and_then(|res| res.json::<RepositoryResponse>())
        {
            Ok(res) => Ok(self.to_repository(&res.name)),
            Err(err) if matches!(err.status(), Some(reqwest::StatusCode::NOT_FOUND)) => {
                Err(ContainerRegistryError::RepositoryDoesntExistInRegistry {
                    registry_name: self.name().to_string(),
                    repository_name: repository_name.to_string(),
                })
            }
            Err(err) => Err(ContainerRegistryError::CannotGetRepository {
                registry_name: self.name().to_string(),
                repository_name: repository_name.to_string(),
                raw_error_message: err.to_string(),
            }),
        }
    }

    fn delete_repository(&self, repository_name: &str) -> Result<(), ContainerRegistryError> {
        let repository_name = self.repository_name_without_namespace(repository_name);

        // https://docs.docker.com/reference/api/hub/latest/#tag/repositories/operation/DeleteNamespaceRepository
        let api_url = format!("{DOCKER_HUB_API_URL}/repositories/{}/{repository_name}/", self.namespace);
        match self
            .http_client
            .delete(api_url)
            .send()
            .and_then(|res| res.error_for_status())
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
        let repository_name = self.repository_name_without_namespace(image.name_without_repository());

        // https://docs.docker.com/reference/api/hub/latest/#tag/repositories/operation/DeleteRepositoryTag
        let api_url = format!(
            "{DOCKER_HUB_API_URL}/repositories/{}/{repository_name}/tags/{}/",
            self.namespace, image.tag
        );
        match self
            .http_client
            .delete(api_url)
            .send()
            .and_then(|res| res.error_for_status())
        {
            Ok(_) => Ok(()),
            Err(err) if matches!(err.status(), Some(reqwest::StatusCode::NOT_FOUND)) => Ok(()),
            Err(err) => Err(ContainerRegistryError::CannotDeleteImage {
                registry_name: self.name().to_string(),
                repository_name: image.repository_name().to_string(),
                image_name: image.name().to_string(),
                raw_error_message: err.to_string(),
            }),
        }
    }

    fn image_exists(&self, image: &Image) -> bool {
        self.generic_cr.image_exists(image)
    }
}

use crate::infrastructure::models::cloud_provider::io::RegistryMirroringMode;

use crate::environment::models::container::get_mirror_repository_name;
use crate::infrastructure::models::container_registry::ContainerRegistryInfo;
use crate::io_models::QoveryIdentifier;
use crate::io_models::container::{DirectPullCredentials, Registry};
use crate::string::cut;
use base64::Engine;
use base64::engine::general_purpose;
use url::Url;
use uuid::Uuid;

// Key kubelet looks up for Docker Hub images, whatever hostname the registry was declared with
const DOCKER_HUB_AUTH_KEY: &str = "https://index.docker.io/v1/";

pub struct RegistryImageSource {
    pub registry: Registry,
    pub image: String,
    pub tag: String,
    pub registry_mirroring_mode: RegistryMirroringMode,
}

impl RegistryImageSource {
    pub fn tag_for_mirror(&self, service_id: &Uuid) -> String {
        // A tag name must be valid ASCII and may contain lowercase and uppercase letters, digits, underscores, periods and dashes.
        // A tag name may not start with a period or a dash and may contain a maximum of 128 characters.
        match self.registry_mirroring_mode {
            RegistryMirroringMode::Service | RegistryMirroringMode::Disabled => {
                cut(format!("{}.{}.{}", self.image.replace('/', "."), self.tag, service_id), 128)
            }
            RegistryMirroringMode::Cluster => cut(format!("{}.{}", self.image.replace('/', "."), self.tag), 128),
        }
    }

    /// Pods pull the image straight from the service registry, without any copy into the cluster registry.
    /// Only when the cluster disables mirroring and the registry credentials outlive the deployment.
    /// Images already on the cluster registry host keep being pulled with the cluster credentials, as core does
    /// not send the credentials of the cluster default registry.
    pub fn is_pulled_from_source(&self, cluster_registry_url: &Url) -> bool {
        let url = self.registry.get_url();
        self.registry_mirroring_mode == RegistryMirroringMode::Disabled
            // kubelet only pulls over https, plain http registries need the mirror
            && url.scheme() == "https"
            && matches!(url.path(), "" | "/")
            // explicit ports only: an http cluster url and an https source on the same host are one registry
            && (url.host_str(), url.port()) != (cluster_registry_url.host_str(), cluster_registry_url.port())
            && self.registry.direct_pull_credentials() != DirectPullCredentials::Unsupported
    }

    /// `host[:port]/image:tag` reference pointing at the service registry
    pub fn source_image_full(&self) -> String {
        format!("{}/{}:{}", self.source_registry_host(), self.image, self.tag)
    }

    /// Base64 docker config json authenticating against the service registry, None for anonymous pulls
    pub fn source_docker_json_config(&self) -> Option<String> {
        let DirectPullCredentials::Static { login, password } = self.registry.direct_pull_credentials() else {
            return None;
        };
        let auth_key = if self.is_docker_hub() {
            DOCKER_HUB_AUTH_KEY.to_string()
        } else {
            self.source_registry_host()
        };
        let auth = general_purpose::STANDARD.encode(format!("{login}:{password}"));
        let docker_json = serde_json::json!({ "auths": { auth_key: { "auth": auth } } });

        Some(general_purpose::STANDARD.encode(docker_json.to_string()))
    }

    fn is_docker_hub(&self) -> bool {
        matches!(self.registry, Registry::DockerHub { .. })
    }

    fn source_registry_host(&self) -> String {
        if self.is_docker_hub() {
            return "docker.io".to_string();
        }
        let url = self.registry.get_url();
        let host = url.host_str().unwrap_or_default().to_lowercase();
        match url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host,
        }
    }

    ///
    /// This method is used to retrieve information about the image used to start the service.
    /// If the service container registry is the same as the cluster container registry url, or if the image is
    /// pulled from its source, no mirroring would be done
    /// The result of this method contains:
    /// * the container registry url pods pull from
    /// * the image name
    /// * the image tag
    /// * a boolean indicating that mirroring must be done
    pub fn compute_cluster_container_registry_url_with_image_name_and_image_tag(
        &self,
        service_id: &Uuid,
        cluster_id: &Uuid,
        cluster_registry_mirroring_mode: &RegistryMirroringMode,
        cluster_registry_info: &ContainerRegistryInfo,
    ) -> (Url, String, String, bool) {
        let cluster_container_registry = cluster_registry_info
            .get_registry_endpoint(Some(QoveryIdentifier::new(*cluster_id).qovery_resource_name()));
        if self.is_pulled_from_source(&cluster_container_registry) {
            return (self.registry.get_url(), self.image.to_string(), self.tag.clone(), false);
        }

        let service_container_registry = self.registry.get_url();

        let cluster_container_registry_host = cluster_container_registry.host_str().unwrap_or_default();
        let service_container_registry_host = service_container_registry.host_str().unwrap_or_default();

        if cluster_container_registry_host == service_container_registry_host {
            (cluster_container_registry, self.image.to_string(), self.tag.clone(), false)
        } else {
            (
                cluster_container_registry,
                cluster_registry_info.get_image_name(&get_mirror_repository_name(
                    service_id,
                    cluster_id,
                    cluster_registry_mirroring_mode,
                )),
                self.tag_for_mirror(service_id),
                true,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io_models::container::Credentials;

    fn a_source(registry: Registry, mode: RegistryMirroringMode) -> RegistryImageSource {
        RegistryImageSource {
            registry,
            image: "didask/api".to_string(),
            tag: "1.2".to_string(),
            registry_mirroring_mode: mode,
        }
    }

    fn a_ghcr(credentials: Option<Credentials>) -> Registry {
        Registry::GenericCr {
            long_id: Uuid::new_v4(),
            url: Url::parse("https://ghcr.io").unwrap(),
            credentials,
        }
    }

    fn credentials() -> Option<Credentials> {
        Some(Credentials {
            login: "user".to_string(),
            password: "t\"oken".to_string(),
        })
    }

    fn decode(value: &str) -> serde_json::Value {
        serde_json::from_slice(&general_purpose::STANDARD.decode(value).unwrap()).unwrap()
    }

    fn cluster_registry() -> Url {
        Url::parse("https://rg.fr-par.scw.cloud/qovery-cluster").unwrap()
    }

    #[test]
    fn should_keep_cluster_credentials_for_images_on_the_cluster_registry_host() {
        let cluster_default_registry = Registry::GenericCr {
            long_id: Uuid::new_v4(),
            url: Url::parse("https://rg.fr-par.scw.cloud").unwrap(),
            credentials: None,
        };

        assert!(
            !a_source(cluster_default_registry, RegistryMirroringMode::Disabled)
                .is_pulled_from_source(&cluster_registry())
        );
    }

    #[test]
    fn should_tell_registries_apart_by_port_on_the_same_host() {
        let cluster_registry = Url::parse("https://registry.example.com:5000").unwrap();
        let on_default_port = Registry::GenericCr {
            long_id: Uuid::new_v4(),
            url: Url::parse("https://registry.example.com").unwrap(),
            credentials: credentials(),
        };
        let on_cluster_port = Registry::GenericCr {
            long_id: Uuid::new_v4(),
            url: Url::parse("https://registry.example.com:5000").unwrap(),
            credentials: credentials(),
        };

        assert!(a_source(on_default_port, RegistryMirroringMode::Disabled).is_pulled_from_source(&cluster_registry));
        assert!(!a_source(on_cluster_port, RegistryMirroringMode::Disabled).is_pulled_from_source(&cluster_registry));
    }

    #[test]
    fn should_match_an_http_cluster_registry_with_an_https_source_on_the_same_host() {
        let cluster_registry = Url::parse("http://registry.corp").unwrap();
        let registry = Registry::GenericCr {
            long_id: Uuid::new_v4(),
            url: Url::parse("https://registry.corp").unwrap(),
            credentials: credentials(),
        };

        assert!(!a_source(registry, RegistryMirroringMode::Disabled).is_pulled_from_source(&cluster_registry));
    }

    #[test]
    fn should_keep_mirroring_plain_http_registries() {
        let registry = Registry::GenericCr {
            long_id: Uuid::new_v4(),
            url: Url::parse("http://registry.internal:5000").unwrap(),
            credentials: credentials(),
        };

        assert!(!a_source(registry, RegistryMirroringMode::Disabled).is_pulled_from_source(&cluster_registry()));
    }

    #[test]
    fn should_pull_from_source_only_when_mirroring_is_disabled() {
        assert!(
            a_source(a_ghcr(credentials()), RegistryMirroringMode::Disabled).is_pulled_from_source(&cluster_registry())
        );
        assert!(
            !a_source(a_ghcr(credentials()), RegistryMirroringMode::Service).is_pulled_from_source(&cluster_registry())
        );
        assert!(
            !a_source(a_ghcr(credentials()), RegistryMirroringMode::Cluster).is_pulled_from_source(&cluster_registry())
        );
    }

    #[test]
    fn should_keep_mirroring_registries_with_temporary_credentials() {
        let ecr = Registry::PrivateEcr {
            long_id: Uuid::new_v4(),
            url: Url::parse("https://123456789012.dkr.ecr.eu-west-3.amazonaws.com").unwrap(),
            region: "eu-west-3".to_string(),
            access_key_id: "key".to_string(),
            secret_access_key: "secret".to_string(),
            session_token: None,
        };
        let azure = Registry::AzureCr {
            long_id: Uuid::new_v4(),
            url: Url::parse("https://acme.azurecr.io").unwrap(),
            credentials: credentials(),
        };
        let gcp = Registry::GcpArtifactRegistry {
            long_id: Uuid::new_v4(),
            url: Url::parse("https://europe-west1-docker.pkg.dev/my-project").unwrap(),
            credentials: Credentials {
                login: "_json_key".to_string(),
                password: "{}".to_string(),
            },
        };

        for registry in [ecr, azure, gcp] {
            assert!(!a_source(registry, RegistryMirroringMode::Disabled).is_pulled_from_source(&cluster_registry()));
        }
    }

    #[test]
    fn should_keep_mirroring_registries_declared_with_a_path() {
        let registry = Registry::GenericCr {
            long_id: Uuid::new_v4(),
            url: Url::parse("https://registry.example.com/team").unwrap(),
            credentials: credentials(),
        };

        assert!(!a_source(registry, RegistryMirroringMode::Disabled).is_pulled_from_source(&cluster_registry()));
    }

    #[test]
    fn should_point_at_the_source_image_when_pulled_from_source() {
        let source = a_source(a_ghcr(credentials()), RegistryMirroringMode::Disabled);

        assert_eq!(source.source_image_full(), "ghcr.io/didask/api:1.2");
    }

    #[test]
    fn should_keep_non_default_port_in_source_image() {
        let registry = Registry::GenericCr {
            long_id: Uuid::new_v4(),
            url: Url::parse("https://Registry.Example.com:5000").unwrap(),
            credentials: None,
        };

        assert_eq!(
            a_source(registry, RegistryMirroringMode::Disabled).source_image_full(),
            "registry.example.com:5000/didask/api:1.2"
        );
    }

    #[test]
    fn should_build_pull_secret_from_source_credentials() {
        let source = a_source(a_ghcr(credentials()), RegistryMirroringMode::Disabled);

        let docker_json = decode(&source.source_docker_json_config().unwrap());

        let expected_auth = general_purpose::STANDARD.encode("user:t\"oken");
        assert_eq!(
            docker_json,
            serde_json::json!({ "auths": { "ghcr.io": { "auth": expected_auth } } })
        );
    }

    #[test]
    fn should_keep_mirroring_docker_hub_with_credentials_injected_by_qovery() {
        let registry = Registry::DockerHub {
            long_id: Uuid::new_v4(),
            url: Url::parse("https://docker.io").unwrap(),
            credentials: credentials(),
            qovery_managed_credentials: true,
        };
        let source = a_source(registry, RegistryMirroringMode::Disabled);

        assert!(!source.is_pulled_from_source(&cluster_registry()));
        assert_eq!(source.source_docker_json_config(), None);
    }

    #[test]
    fn should_read_qovery_managed_credentials_flag_from_core_payload() {
        let with_flag: Registry = serde_json::from_str(
            r#"{"DockerHub": {"long_id": "00000000-0000-0000-0000-000000000000", "url": "https://docker.io", "credentials": {"login": "pool", "password": "token"}, "qovery_managed_credentials": true}}"#,
        )
        .unwrap();
        let without_flag: Registry = serde_json::from_str(
            r#"{"DockerHub": {"long_id": "00000000-0000-0000-0000-000000000000", "url": "https://docker.io", "credentials": null}}"#,
        )
        .unwrap();

        assert!(with_flag.has_qovery_managed_credentials());
        assert!(!without_flag.has_qovery_managed_credentials());
    }

    #[test]
    fn should_use_the_canonical_docker_hub_key_in_pull_secret() {
        let registry = Registry::DockerHub {
            long_id: Uuid::new_v4(),
            url: Url::parse("https://docker.io").unwrap(),
            credentials: credentials(),
            qovery_managed_credentials: false,
        };
        let source = a_source(registry, RegistryMirroringMode::Disabled);

        let docker_json = decode(&source.source_docker_json_config().unwrap());

        assert_eq!(source.source_image_full(), "docker.io/didask/api:1.2");
        assert!(docker_json["auths"][DOCKER_HUB_AUTH_KEY]["auth"].is_string());
    }

    #[test]
    fn should_not_build_pull_secret_for_anonymous_registries() {
        let source = a_source(a_ghcr(None), RegistryMirroringMode::Disabled);

        assert!(source.is_pulled_from_source(&cluster_registry()));
        assert_eq!(source.source_docker_json_config(), None);
    }
}

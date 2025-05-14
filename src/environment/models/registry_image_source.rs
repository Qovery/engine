use crate::infrastructure::models::cloud_provider::io::RegistryMirroringMode;

use crate::environment::models::container::get_mirror_repository_name;
use crate::infrastructure::models::container_registry::ContainerRegistryInfo;
use crate::io_models::QoveryIdentifier;
use crate::io_models::container::Registry;
use crate::string::cut;
use url::Url;
use uuid::Uuid;

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
            RegistryMirroringMode::Service => {
                cut(format!("{}.{}.{}", self.image.replace('/', "."), self.tag, service_id), 128)
            }
            RegistryMirroringMode::Cluster => cut(format!("{}.{}", self.image.replace('/', "."), self.tag), 128),
        }
    }

    ///
    /// This method is used to retrieve information about the image used to start the service.
    /// If the service container registry is the same as the cluster container registry url, no mirroring would be done
    /// The result of this method contains:
    /// * the cluster container registry url
    /// * the cluster image name
    /// * the cluster image tag
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

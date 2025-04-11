use super::blob_storage_regions::AzureStorageRegion;
use crate::infrastructure::models::container_registry::Repository;
use crate::infrastructure::models::object_storage::{Bucket, BucketRegion};
use azure_storage_blobs::container::Container;
use serde_derive::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

pub const AZURE_STORAGE_METADATA_PREFIX: &str = "x-ms-meta-";

impl Bucket {
    pub fn from_azure_container(container: Container, location: AzureStorageRegion) -> Result<Self, String> {
        let metadata_without_prefix: HashMap<String, String> = container
            .metadata
            .iter()
            .map(|(k, v)| {
                (
                    k.strip_prefix(AZURE_STORAGE_METADATA_PREFIX).unwrap_or(k).to_string(),
                    v.to_string(),
                )
            })
            .collect();

        let mut ttl = None;
        if let Some(ttl_str) = metadata_without_prefix.get("ttl") {
            if let Ok(ttl_secs) = ttl_str.parse::<u64>() {
                ttl = Some(Duration::from_secs(ttl_secs));
            }
        }

        Ok(Bucket {
            name: container.name,
            ttl,
            versioning_activated: false, // TODO(benjaminch): handle bucket versioning
            logging_activated: false,    // TODO(benjaminch): handle bucket logging
            location: BucketRegion::AzureRegion(location.clone()),
            labels: match metadata_without_prefix.is_empty() {
                false => Some(metadata_without_prefix),
                true => None,
            },
        })
    }
}

pub fn from_azure_container_registry(
    azure_container_registry: azure_mgmt_containerregistry::models::Registry,
) -> Result<Repository, String> {
    Ok(Repository {
        registry_id: azure_container_registry.resource.id.unwrap_or_default(),
        name: azure_container_registry.resource.name.unwrap_or_default(),
        uri: azure_container_registry.properties.unwrap_or_default().login_server,
        ttl: None,    // TODO(benjaminch): TTL to be added
        labels: None, // TODO(benjaminch): labels to be added
    })
}

// Output of the Azure CLI Docker Image Tag
// {
//   "changeableAttributes": {
//     "deleteEnabled": true,
//     "listEnabled": true,
//     "readEnabled": true,
//     "writeEnabled": true
//   },
//   "createdTime": "2025-04-09T15:25:17.3560664Z",
//   "digest": "sha256:92c7f9c92844bbbb5d0a101b22f7c2a7949e40f8ea90c8b3bc396879d95e899a",
//   "lastUpdateTime": "2025-04-09T15:25:17.3560664Z",
//   "name": "v1",
//   "signed": false
// }
#[derive(Deserialize, Debug)]
pub struct DockerImageTag {
    pub name: String,
}

use super::blob_storage_regions::AzureStorageRegion;
use crate::infrastructure::models::object_storage::{Bucket, BucketRegion};
use azure_storage_blobs::container::Container;
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

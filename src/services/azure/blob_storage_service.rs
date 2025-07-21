use crate::infrastructure::models::object_storage::{Bucket, BucketObject, BucketRegion};
use crate::runtime::block_on;
use crate::services::azure::azure_cloud_sdk_types::AZURE_STORAGE_METADATA_PREFIX;
use crate::services::azure::blob_storage_regions::{AzureStorageRegion, CloudLocationWrapper};
use azure_core::headers::{HeaderName, HeaderValue, Headers};
use azure_storage::StorageCredentials;
use azure_storage_blobs::container::operations::BlobItem;
use azure_storage_blobs::prelude::ClientBuilder;
use chrono::Utc;
use futures::StreamExt;
use std::cmp::max;
use std::collections::HashMap;
use std::time::Duration;
use thiserror::Error;

#[derive(Clone, Error, Debug, PartialEq, Eq)]
pub enum BlobStorageServiceError {
    #[error("Cannot create bucket `{bucket_name}` in storage account `{storage_account_name}`: {raw_error_message:?}")]
    CannotCreateBucket {
        bucket_name: String,
        storage_account_name: String,
        raw_error_message: String,
    },
    #[error("Cannot update bucket `{bucket_name}` in storage account `{storage_account_name}`: {raw_error_message:?}")]
    CannotUpdateBucket {
        bucket_name: String,
        storage_account_name: String,
        raw_error_message: String,
    },
    #[error(
        "Cannot delete bucket `{bucket_name}` from storage account `{storage_account_name}`: {raw_error_message:?}"
    )]
    CannotDeleteBucket {
        bucket_name: String,
        storage_account_name: String,
        raw_error_message: String,
    },
    #[error("Cannot get bucket `{bucket_name}` from storage account `{storage_account_name}`: {raw_error_message:?}")]
    CannotGetBucket {
        bucket_name: String,
        storage_account_name: String,
        raw_error_message: String,
    },
    #[error(
        "Cannot check if bucket `{bucket_name}` exists in storage account `{storage_account_name}`: {raw_error_message:?}"
    )]
    CannotCheckIfBucketExists {
        bucket_name: String,
        storage_account_name: String,
        raw_error_message: String,
    },
    #[error(
        "Cannot put object `{object_key}` to bucket `{bucket_name}` in storage account `{storage_account_name}`: {raw_error_message:?}"
    )]
    CannotPutObjectToBucket {
        storage_account_name: String,
        object_key: String,
        bucket_name: String,
        raw_error_message: String,
    },
    #[error(
        "Cannot get object `{object_key}` from bucket `{bucket_name}` from storage account `{storage_account_name}`: {raw_error_message:?}"
    )]
    CannotGetObject {
        storage_account_name: String,
        object_key: String,
        bucket_name: String,
        raw_error_message: String,
    },
    #[error(
        "Cannot get object properties `{object_key}` from bucket `{bucket_name}` from storage account `{storage_account_name}`: {raw_error_message:?}"
    )]
    CannotGetObjectProperties {
        storage_account_name: String,
        object_key: String,
        bucket_name: String,
        raw_error_message: String,
    },
    #[error(
        "Cannot delete object `{object_key}` from bucket `{bucket_name}` from storage account `{storage_account_name}`: {raw_error_message:?}"
    )]
    CannotDeleteObject {
        storage_account_name: String,
        object_key: String,
        bucket_name: String,
        raw_error_message: String,
    },
    #[error(
        "Cannot list objects from bucket `{bucket_name}` from storage account `{storage_account_name}`: {raw_error_message:?}"
    )]
    CannotListObjects {
        storage_account_name: String,
        bucket_name: String,
        raw_error_message: String,
    },
    #[error("Cannot list buckets from storage account `{storage_account_name}`: {raw_error_message:?}")]
    CannotListBuckets {
        storage_account_name: String,
        raw_error_message: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageAccount {
    pub access_key: String,
    pub account_name: String,
}

#[cfg_attr(test, faux::create)]
pub struct BlobStorageService {}

#[cfg_attr(test, faux::methods)]
impl BlobStorageService {
    pub fn new() -> Self {
        Self {}
    }

    fn client(&self, storage_account: &StorageAccount) -> ClientBuilder {
        let storage_credentials =
            StorageCredentials::access_key(&storage_account.account_name, storage_account.access_key.to_string());
        ClientBuilder::new(&storage_account.account_name, storage_credentials) // TODO(benjaminch): to be improved
    }

    pub fn bucket_exists(
        &self,
        storage_account: &StorageAccount,
        bucket_name: &str,
        bucket_location: &AzureStorageRegion,
    ) -> bool {
        let client = self.client(storage_account);
        let container_client = client
            .cloud_location(CloudLocationWrapper::from(bucket_location.clone()).to_owned())
            .container_client(bucket_name);

        block_on(container_client.exists()).unwrap_or(false)
    }

    pub fn create_bucket(
        &self,
        storage_account: &StorageAccount,
        bucket_name: &str,
        bucket_location: &AzureStorageRegion,
        bucket_ttl: Option<Duration>,
        // _bucket_versioning_activated: bool, // TODO(benjaminch): implement the bucket_versioning_activated
        // _bucket_logging_activated: bool,    // TODO(benjaminch): implement the bucket_logging_activated
        bucket_labels: Option<HashMap<String, String>>,
    ) -> Result<Bucket, BlobStorageServiceError> {
        // Recreate the ClientBuilder when needed
        let client = self.client(storage_account);
        let container_client = client
            .cloud_location(CloudLocationWrapper::from(bucket_location.clone()).to_owned())
            .container_client(bucket_name);

        let bucket_ttl = bucket_ttl.map(|ttl| max(ttl, Duration::from_secs(60 * 60 * 24)));

        let mut bucket_labels = bucket_labels.unwrap_or_default();
        *bucket_labels
            .entry("creation_date".to_string())
            .or_insert(Utc::now().timestamp().to_string()) = Utc::now().timestamp().to_string();
        if let Some(bucket_ttl) = bucket_ttl {
            let ttl = bucket_ttl.as_secs();
            *bucket_labels.entry("ttl".to_string()).or_insert(ttl.to_string()) = ttl.to_string();
        }

        // TODO(benjaminch): implement the bucket_ttl, bucket_versioning_activated, bucket_logging_activated, bucket_labels
        let mut metadata = HashMap::new();
        for (key, value) in bucket_labels.iter() {
            metadata.insert(
                // adding `x-ms-meta-` prefix to header name is a dirty hack to be able to have
                // headers properly populated from.
                // If this prefix is not there, the header will not be added to the metadata
                // CF `impl From<&Headers> for Metadata` implementation in metadata.rs
                // ```
                //  impl From<&Headers> for Metadata {
                //      fn from(header_map: &Headers) -> Self {
                //          let mut metadata = Metadata::new();
                //          header_map.iter().for_each(|(name, value)| {
                //              let name = name.as_str();
                //              let value = value.as_str();
                //              if let Some(name) = name.strip_prefix("x-ms-meta-") {
                //                  metadata.insert(name.to_owned(), value.to_owned());
                //              }
                //          });

                //          metadata
                //      }
                //  }
                // ```
                // Opened an issue to fix this: https://github.com/Azure/azure-sdk-for-rust/issues/2289
                HeaderName::from(format!("{AZURE_STORAGE_METADATA_PREFIX}{key}")),
                HeaderValue::from(value.to_string()),
            );
        }

        match block_on(
            container_client
                .create()
                // No public access by default
                .public_access(azure_storage_blobs::container::PublicAccess::None)
                .metadata(&Headers::from(metadata))
                .into_future(),
        ) {
            Ok(_) => Ok(Bucket {
                name: bucket_name.to_string(),
                location: BucketRegion::AzureRegion(bucket_location.clone()),
                ttl: bucket_ttl,
                versioning_activated: false,
                logging_activated: false,
                labels: Some(bucket_labels),
            }),
            Err(e) => Err(BlobStorageServiceError::CannotCreateBucket {
                storage_account_name: storage_account.account_name.to_string(),
                bucket_name: bucket_name.to_string(),
                raw_error_message: e.to_string(),
            }),
        }
    }

    pub fn delete_bucket(
        &self,
        storage_account: &StorageAccount,
        bucket_name: &str,
    ) -> Result<(), BlobStorageServiceError> {
        let client = self.client(storage_account);
        let container_client = client.container_client(bucket_name);

        match block_on(container_client.delete().into_future()) {
            Ok(_) => Ok(()),
            Err(e) => Err(BlobStorageServiceError::CannotDeleteBucket {
                storage_account_name: storage_account.account_name.to_string(),
                bucket_name: bucket_name.to_string(),
                raw_error_message: e.to_string(),
            }),
        }
    }

    pub fn get_bucket(
        &self,
        storage_account: &StorageAccount,
        bucket_name: &str,
        bucket_location: &AzureStorageRegion,
    ) -> Result<Bucket, BlobStorageServiceError> {
        let client = self.client(storage_account);
        let container_client = client.container_client(bucket_name);

        match block_on(container_client.get_properties().into_future()) {
            Ok(container_response) => Ok(Bucket::from_azure_container(
                container_response.container,
                bucket_location.clone(),
            )
            .map_err(|e| BlobStorageServiceError::CannotGetBucket {
                storage_account_name: storage_account.account_name.to_string(),
                bucket_name: bucket_name.to_string(),
                raw_error_message: e.to_string(),
            })?),
            Err(e) => Err(BlobStorageServiceError::CannotGetBucket {
                storage_account_name: storage_account.account_name.to_string(),
                bucket_name: bucket_name.to_string(),
                raw_error_message: e.to_string(),
            }),
        }
    }

    pub fn list_buckets(
        &self,
        storage_account: &StorageAccount,
        bucket_location: &AzureStorageRegion,
        bucket_name_prefix: Option<&str>,
        bucket_labels: Option<HashMap<String, String>>,
    ) -> Result<Vec<Bucket>, BlobStorageServiceError> {
        let client = self.client(storage_account);
        let blob_client = client.blob_service_client();

        let mut stream = blob_client.list_containers().into_stream();

        let mut buckets = Vec::new();

        while let Some(value) = block_on(stream.next()) {
            let containers = value
                .map_err(|e| BlobStorageServiceError::CannotListBuckets {
                    storage_account_name: storage_account.account_name.to_string(),
                    raw_error_message: e.to_string(),
                })?
                .containers;
            for container in containers {
                if let Some(bucket_name_prefix) = bucket_name_prefix {
                    if !container.name.starts_with(bucket_name_prefix) {
                        continue;
                    }
                }
                if let Some(bucket_labels) = &bucket_labels {
                    for (key, value) in bucket_labels.iter() {
                        if container.metadata.get(key).map(|v| v.as_str()) != Some(value) {
                            continue;
                        }
                    }
                }

                buckets.push(Bucket::from_azure_container(container, bucket_location.clone()).map_err(|e| {
                    BlobStorageServiceError::CannotListBuckets {
                        storage_account_name: storage_account.account_name.to_string(),
                        raw_error_message: e.to_string(),
                    }
                })?);
            }
        }

        Ok(buckets)
    }

    pub fn update_bucket(
        &self,
        _storage_account: &StorageAccount,
        _bucket_name: &str,
        _bucket_location: &AzureStorageRegion,
        _bucket_ttl: Option<Duration>,
        _bucket_versioning_activated: bool,
        _bucket_logging_activated: bool,
        _bucket_labels: Option<HashMap<String, String>>,
    ) -> Result<Bucket, BlobStorageServiceError> {
        // Retrieve the current bucket
        // let _existing_bucket = self.get_bucket(bucket_name, bucket_location)?;

        // let client = self.client();
        // let container_client = client.container_client(bucket_name);

        // let bucket_ttl = bucket_ttl.map(|ttl| max(ttl, Duration::from_secs(60 * 60 * 24)));

        // let mut bucket_labels = bucket_labels.unwrap_or_default();
        // *bucket_labels
        //     .entry("creation_date".to_string())
        //     .or_insert(Utc::now().timestamp().to_string()) = Utc::now().timestamp().to_string();
        // if let Some(bucket_ttl) = bucket_ttl {
        //     let ttl = bucket_ttl.as_secs();
        //     *bucket_labels.entry("ttl".to_string()).or_insert(ttl.to_string()) = ttl.to_string();
        // }

        todo!("Implement the update bucket logic, not available on the SDK as of today")
    }

    pub fn get_object(
        &self,
        storage_account: &StorageAccount,
        bucket_name: &str,
        object_key: &str,
    ) -> Result<BucketObject, BlobStorageServiceError> {
        let client = self.client(storage_account);
        let container_client = client.container_client(bucket_name);
        let blob_client = container_client.blob_client(object_key);

        let mut blob_content: Vec<u8> = vec![];

        // The stream is composed of individual calls to the get blob endpoint
        let mut stream = blob_client.get().into_stream();

        while let Some(value) = block_on(stream.next()) {
            let mut body = value
                .map_err(|e| BlobStorageServiceError::CannotGetBucket {
                    storage_account_name: storage_account.account_name.to_string(),
                    bucket_name: bucket_name.to_string(),
                    raw_error_message: e.to_string(),
                })?
                .data;
            while let Some(value) = block_on(body.next()) {
                let value = value.map_err(|e| BlobStorageServiceError::CannotGetBucket {
                    storage_account_name: storage_account.account_name.to_string(),
                    bucket_name: bucket_name.to_string(),
                    raw_error_message: e.to_string(),
                })?;
                blob_content.extend(&value);
            }
        }

        Ok(BucketObject {
            bucket_name: bucket_name.to_string(),
            key: object_key.to_string(),
            value: blob_content,
            tags: match block_on(blob_client.get_properties().into_future()) {
                Ok(blob_properties) => blob_properties
                    .blob
                    .metadata
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect(),
                Err(e) => {
                    return Err(BlobStorageServiceError::CannotGetObjectProperties {
                        storage_account_name: storage_account.account_name.to_string(),
                        bucket_name: bucket_name.to_string(),
                        object_key: object_key.to_string(),
                        raw_error_message: e.to_string(),
                    });
                }
            },
        })
    }

    pub fn put_object(
        &self,
        storage_account: &StorageAccount,
        bucket_name: &str,
        object_key: &str,
        content: Vec<u8>,
        object_labels: Option<HashMap<String, String>>,
    ) -> Result<BucketObject, BlobStorageServiceError> {
        let client = self.client(storage_account);
        let container_client = client.container_client(bucket_name);
        let blob_client = container_client.blob_client(object_key);

        // TODO(benjaminch): implement the bucket_ttl, bucket_versioning_activated, bucket_logging_activated, bucket_labels
        let mut metadata = Headers::new();
        for (key, value) in object_labels.unwrap_or_default().iter() {
            metadata.insert(
                // adding `x-ms-meta-` prefix to header name is a dirty hack to be able to have
                // headers properly populated from.
                // If this prefix is not there, the header will not be added to the metadata
                // CF `impl From<&Headers> for Metadata` implementation in metadata.rs
                // ```
                //  impl From<&Headers> for Metadata {
                //      fn from(header_map: &Headers) -> Self {
                //          let mut metadata = Metadata::new();
                //          header_map.iter().for_each(|(name, value)| {
                //              let name = name.as_str();
                //              let value = value.as_str();
                //              if let Some(name) = name.strip_prefix("x-ms-meta-") {
                //                  metadata.insert(name.to_owned(), value.to_owned());
                //              }
                //          });

                //          metadata
                //      }
                //  }
                // ```
                // Opened an issue to fix this: https://github.com/Azure/azure-sdk-for-rust/issues/2289
                HeaderName::from(format!("{AZURE_STORAGE_METADATA_PREFIX}{key}")),
                HeaderValue::from(value.to_string()),
            );
        }

        match block_on(blob_client.put_block_blob(content).metadata(&metadata).into_future()) {
            Ok(_) => self.get_object(storage_account, bucket_name, object_key),
            Err(e) => Err(BlobStorageServiceError::CannotPutObjectToBucket {
                storage_account_name: storage_account.account_name.to_string(),
                bucket_name: bucket_name.to_string(),
                object_key: object_key.to_string(),
                raw_error_message: e.to_string(),
            }),
        }
    }

    pub fn list_objects(
        &self,
        storage_account: &StorageAccount,
        bucket_name: &str,
        object_id_prefix: Option<&str>,
        object_labels: Option<HashMap<String, String>>,
    ) -> Result<Vec<BucketObject>, BlobStorageServiceError> {
        let client = self.client(storage_account);
        let container_client = client.container_client(bucket_name);

        let mut stream = container_client.list_blobs().into_stream();

        let mut objects = Vec::new();

        while let Some(value) = block_on(stream.next()) {
            let blobs = value
                .map_err(|e| BlobStorageServiceError::CannotListObjects {
                    storage_account_name: storage_account.account_name.to_string(),
                    bucket_name: bucket_name.to_string(),
                    raw_error_message: e.to_string(),
                })?
                .blobs;
            'blobs: for blob in blobs.items {
                if let BlobItem::Blob(b) = blob {
                    let object_key = b.name;
                    if let Some(object_id_prefix) = object_id_prefix {
                        if !object_key.starts_with(object_id_prefix) {
                            continue;
                        }
                    }
                    if let Some(object_labels) = &object_labels {
                        if let Some(metadata) = b.metadata {
                            for (key, value) in object_labels.iter() {
                                if metadata.get(key).map(|v| v.as_str()) != Some(value) {
                                    continue 'blobs;
                                }
                            }
                        }
                    }
                    let object = self.get_object(storage_account, bucket_name, &object_key)?;
                    objects.push(object);
                }
            }
        }

        Ok(objects)
    }

    pub fn delete_object(
        &self,
        storage_account: &StorageAccount,
        bucket_name: &str,
        object_key: &str,
    ) -> Result<(), BlobStorageServiceError> {
        let client = self.client(storage_account);
        let container_client = client.container_client(bucket_name);
        let blob_client = container_client.blob_client(object_key);

        match block_on(blob_client.delete().into_future()) {
            Ok(_) => Ok(()),
            Err(e) => Err(BlobStorageServiceError::CannotDeleteObject {
                storage_account_name: storage_account.account_name.to_string(),
                bucket_name: bucket_name.to_string(),
                object_key: object_key.to_string(),
                raw_error_message: e.to_string(),
            }),
        }
    }
}

#[cfg_attr(test, faux::methods)]
impl Default for BlobStorageService {
    fn default() -> Self {
        Self::new()
    }
}

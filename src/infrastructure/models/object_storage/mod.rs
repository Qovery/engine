use crate::environment::models::ToCloudProviderFormat;
use crate::environment::models::scaleway::ScwZone;
use crate::infrastructure::models::cloud_provider::aws::regions::AwsRegion;
use crate::infrastructure::models::object_storage::errors::ObjectStorageError;
use crate::services::azure::blob_storage_regions::AzureStorageRegion;
use crate::services::gcp::object_storage_regions::GcpStorageRegion;
use enum_dispatch::enum_dispatch;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use strum_macros::EnumIter;

pub mod azure_object_storage;
pub mod errors;
pub mod google_object_storage;
pub mod s3;
pub mod scaleway_object_storage;

#[derive(Clone, Debug, EnumIter, PartialEq)]
pub enum BucketDeleteStrategy {
    HardDelete,
    Empty,
}

#[enum_dispatch]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BucketRegion {
    AwsRegion(AwsRegion),
    AzureRegion(AzureStorageRegion),
    ScwRegion(ScwZone),
    GcpRegion(GcpStorageRegion),
}

#[enum_dispatch(StorageRegion)]
pub trait StorageRegion: ToCloudProviderFormat {}

pub trait ObjectStorage {
    fn kind(&self) -> Kind;
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn name_with_id(&self) -> String {
        format!("{} ({})", self.name(), self.id())
    }
    fn is_valid(&self) -> Result<(), ObjectStorageError>;
    fn workspace_dir_relative_path(&self) -> String {
        "object-storage/s3".to_string()
    }
    fn bucket_exists(&self, bucket_name: &str) -> bool;
    fn create_bucket(
        &self,
        bucket_name: &str,
        bucket_ttl: Option<Duration>,
        bucket_versioning_activated: bool,
        bucket_logging_activated: bool,
    ) -> Result<Bucket, ObjectStorageError>;
    fn update_bucket(
        &self,
        bucket_name: &str,
        bucket_ttl: Option<Duration>,
        bucket_versioning_activated: bool,
        bucket_logging_activated: bool,
        bucket_labels: Option<HashMap<String, String>>,
    ) -> Result<Bucket, ObjectStorageError>;
    fn get_bucket(&self, bucket_name: &str) -> Result<Bucket, ObjectStorageError>;
    fn delete_bucket(
        &self,
        bucket_name: &str,
        bucket_delete_strategy: BucketDeleteStrategy,
    ) -> Result<(), ObjectStorageError>;
    fn delete_bucket_non_blocking(&self, bucket_name: &str) -> Result<(), ObjectStorageError>;
    fn get_object(&self, bucket_name: &str, object_key: &str) -> Result<BucketObject, ObjectStorageError>;
    fn put_object(
        &self,
        bucket_name: &str,
        object_key: &str,
        file_path: &Path,
        tags: Option<Vec<String>>,
    ) -> Result<BucketObject, ObjectStorageError>;
    fn delete_object(&self, bucket_name: &str, object_key: &str) -> Result<(), ObjectStorageError>;
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Kind {
    S3,
    Spaces,
    ScalewayOs,
    GcpOs,
    AzureOs,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bucket {
    pub name: String,
    pub ttl: Option<Duration>,
    pub versioning_activated: bool,
    pub logging_activated: bool,
    pub location: BucketRegion,
    pub labels: Option<HashMap<String, String>>,
}

impl Bucket {
    pub fn new(
        name: String,
        ttl: Option<Duration>,
        versioning_activated: bool,
        logging_activated: bool,
        location: BucketRegion,
        labels: Option<HashMap<String, String>>,
    ) -> Self {
        Self {
            name,
            ttl,
            versioning_activated,
            logging_activated,
            location,
            labels,
        }
    }

    pub fn generate_logging_bucket_name_for_bucket(bucket_name: &str) -> String {
        format!("{bucket_name}-log")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BucketObject {
    pub bucket_name: String,
    pub key: String,
    pub value: Vec<u8>,
    pub tags: Vec<String>,
}

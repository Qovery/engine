use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::io_models::context::Context;
use crate::models::domain::StringPath;
use crate::models::ToCloudProviderFormat;
use crate::object_storage::errors::ObjectStorageError;
use chrono::Duration;
use std::fs::File;

pub mod errors;
pub mod google_object_storage;
pub mod s3;
pub mod scaleway_object_storage;

pub trait ObjectStorage {
    fn context(&self) -> &Context;
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
    fn workspace_dir_full_path(&self) -> String {
        format!(
            "{}/.qovery-workspace/{}/{}",
            self.context().workspace_root_dir(),
            self.context().execution_id(),
            self.workspace_dir_relative_path()
        )
    }
    fn create_bucket(&self, bucket_name: &str) -> Result<(), ObjectStorageError>;
    fn delete_bucket(&self, bucket_name: &str) -> Result<(), ObjectStorageError>;
    fn get(
        &self,
        bucket_name: &str,
        object_key: &str,
        use_cache: bool,
    ) -> Result<(StringPath, File), ObjectStorageError>;
    fn put(&self, bucket_name: &str, object_key: &str, file_path: &str) -> Result<(), ObjectStorageError>;
    fn delete(&self, bucket_name: &str, object_key: &str) -> Result<(), ObjectStorageError>;
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Kind {
    S3,
    Spaces,
    ScalewayOs,
    GcpOs,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bucket<R>
where
    R: ToCloudProviderFormat,
{
    pub name: String,
    pub ttl: Option<Duration>,
    pub location: R,
    pub labels: Option<HashMap<String, String>>,
}

impl<R> Bucket<R>
where
    R: ToCloudProviderFormat,
{
    pub fn new(name: String, ttl: Option<Duration>, location: R, labels: Option<HashMap<String, String>>) -> Self {
        Self {
            name,
            ttl,
            location,
            labels,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct BucketObject {
    pub bucket_name: String,
    pub key: String,
    pub value: Vec<u8>,
}

use crate::cloud_provider::gcp::locations::GcpRegion as GcpCloudJobRegion;
use crate::models::gcp::JsonCredentials;
use crate::models::ToCloudProviderFormat;
use crate::object_storage::{Bucket, BucketObject};
use crate::runtime::block_on;
use crate::services::gcp::cloud_job_service::CloudJobService;
use crate::services::gcp::google_cloud_sdk_types::new_gcp_credentials_file_from_credentials;
use crate::services::gcp::object_storage_regions::GcpStorageRegion;
use google_cloud_storage::client::{Client, ClientConfig};
use google_cloud_storage::http::buckets::delete::DeleteBucketRequest;
use google_cloud_storage::http::buckets::get::GetBucketRequest;
use google_cloud_storage::http::buckets::insert::{BucketCreationConfig, InsertBucketParam, InsertBucketRequest};
use google_cloud_storage::http::buckets::lifecycle::rule::{Action, ActionType, Condition};
use google_cloud_storage::http::buckets::lifecycle::Rule;
use google_cloud_storage::http::buckets::list::ListBucketsRequest;
use google_cloud_storage::http::buckets::patch::{BucketPatchConfig, PatchBucketRequest};
use google_cloud_storage::http::buckets::Lifecycle;
use google_cloud_storage::http::buckets::{Bucket as GcpBucket, Versioning};
use google_cloud_storage::http::objects::delete::DeleteObjectRequest;
use google_cloud_storage::http::objects::download::Range;
use google_cloud_storage::http::objects::get::GetObjectRequest;
use google_cloud_storage::http::objects::list::ListObjectsRequest;
use google_cloud_storage::http::objects::upload::{UploadObjectRequest, UploadType};
use google_cloud_storage::http::objects::Object as GcpObject;
use governor::middleware::NoOpMiddleware;
use governor::state::{InMemoryState, NotKeyed};
use governor::{clock, RateLimiter};

use reqwest::Body;
use std::cmp::max;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;

#[derive(Clone, Error, Debug, PartialEq, Eq)]
pub enum ObjectStorageServiceError {
    #[error("Cannot create object storage service: {raw_error_message:?}")]
    CannotCreateService { raw_error_message: String },
    #[error("Cannot create bucket `{bucket_name}`: {raw_error_message:?}")]
    CannotCreateBucket {
        bucket_name: String,
        raw_error_message: String,
    },
    #[error("Cannot update bucket `{bucket_name}`: {raw_error_message:?}")]
    CannotUpdateBucket {
        bucket_name: String,
        raw_error_message: String,
    },
    #[error("Cannot get bucket `{bucket_name}`: {raw_error_message:?}")]
    CannotGetBucket {
        bucket_name: String,
        raw_error_message: String,
    },
    #[error("Cannot delete bucket `{bucket_name}`: {raw_error_message:?}")]
    CannotDeleteBucket {
        bucket_name: String,
        raw_error_message: String,
    },
    #[error("Cannot delete object `{object_id}` from bucket `{bucket_name}`: {raw_error_message:?}")]
    CannotDeleteObject {
        bucket_name: String,
        object_id: String,
        raw_error_message: String,
    },
    #[error("Cannot list buckets: {raw_error_message:?}")]
    CannotListBuckets { raw_error_message: String },
    #[error("Cannot list objects from bucket `{bucket_name}`: {raw_error_message:?}")]
    CannotListObjects {
        bucket_name: String,
        raw_error_message: String,
    },
    #[error("Cannot put object `{object_key}` to bucket `{bucket_name}`: {raw_error_message:?}")]
    CannotPutObjectToBucket {
        object_key: String,
        bucket_name: String,
        raw_error_message: String,
    },
    #[error("Cannot get object `{object_key}` from bucket `{bucket_name}`: {raw_error_message:?}")]
    CannotGetObject {
        object_key: String,
        bucket_name: String,
        raw_error_message: String,
    },
    #[error("Cannot proceed, admission control blocked after several tries")]
    AdmissionControlCannotProceedAfterSeveralTries,
}

impl ObjectStorageServiceError {
    pub fn get_raw_error_message(self) -> String {
        match self {
            ObjectStorageServiceError::CannotCreateService { raw_error_message } => raw_error_message,
            ObjectStorageServiceError::CannotCreateBucket { raw_error_message, .. } => raw_error_message,
            ObjectStorageServiceError::CannotUpdateBucket { raw_error_message, .. } => raw_error_message,
            ObjectStorageServiceError::CannotGetBucket { raw_error_message, .. } => raw_error_message,
            ObjectStorageServiceError::CannotDeleteBucket { raw_error_message, .. } => raw_error_message,
            ObjectStorageServiceError::CannotDeleteObject { raw_error_message, .. } => raw_error_message,
            ObjectStorageServiceError::CannotListBuckets { raw_error_message, .. } => raw_error_message,
            ObjectStorageServiceError::CannotListObjects { raw_error_message, .. } => raw_error_message,
            ObjectStorageServiceError::CannotPutObjectToBucket { raw_error_message, .. } => raw_error_message,
            ObjectStorageServiceError::CannotGetObject { raw_error_message, .. } => raw_error_message,
            ObjectStorageServiceError::AdmissionControlCannotProceedAfterSeveralTries => "".to_string(),
        }
    }
}

enum StorageResourceKind {
    Bucket,
    Object,
}

#[cfg_attr(test, faux::create)]
pub struct ObjectStorageService {
    client: Client,
    client_email: String,
    project_id: String,
    write_bucket_rate_limiter: Option<Arc<RateLimiter<NotKeyed, InMemoryState, clock::DefaultClock, NoOpMiddleware>>>,
    write_object_rate_limiter: Option<Arc<RateLimiter<NotKeyed, InMemoryState, clock::DefaultClock, NoOpMiddleware>>>,
    cloud_job_service: Arc<CloudJobService>,
}

#[cfg_attr(test, faux::methods)]
impl ObjectStorageService {
    pub fn new(
        google_credentials: JsonCredentials,
        bucket_rate_limiter: Option<Arc<RateLimiter<NotKeyed, InMemoryState, clock::DefaultClock, NoOpMiddleware>>>,
        object_rate_limiter: Option<Arc<RateLimiter<NotKeyed, InMemoryState, clock::DefaultClock, NoOpMiddleware>>>,
    ) -> Result<Self, ObjectStorageServiceError> {
        Ok(Self {
            client: Client::new(
                block_on(ClientConfig::default().with_credentials(
                    new_gcp_credentials_file_from_credentials(google_credentials.clone()).map_err(|e| {
                        ObjectStorageServiceError::CannotCreateService {
                            raw_error_message: e.to_string(),
                        }
                    })?,
                ))
                .map_err(|e| ObjectStorageServiceError::CannotCreateService {
                    raw_error_message: e.to_string(),
                })?,
            ),
            write_bucket_rate_limiter: bucket_rate_limiter,
            write_object_rate_limiter: object_rate_limiter,
            client_email: google_credentials.client_email.to_string(),
            project_id: google_credentials.project_id.to_string(),
            cloud_job_service: Arc::from(CloudJobService::new(google_credentials).map_err(|e| {
                ObjectStorageServiceError::CannotCreateService {
                    raw_error_message: e.to_string(),
                }
            })?),
        })
    }

    fn wait_for_a_slot_in_admission_control(
        &self,
        timeout: Duration,
        resource_kind: StorageResourceKind,
    ) -> Result<(), ObjectStorageServiceError> {
        if let Some(rate_limiter) = match resource_kind {
            StorageResourceKind::Bucket => &self.write_bucket_rate_limiter,
            StorageResourceKind::Object => &self.write_object_rate_limiter,
        } {
            let start = Instant::now();

            loop {
                if start.elapsed() > timeout {
                    return Err(ObjectStorageServiceError::AdmissionControlCannotProceedAfterSeveralTries);
                }

                if rate_limiter.check().is_err() {
                    std::thread::sleep(Duration::from_secs(3));
                    continue;
                }

                break;
            }
        }

        Ok(())
    }

    pub fn bucket_exists(&self, bucket_name: &str) -> bool {
        self.get_bucket(bucket_name).is_ok()
    }

    pub fn get_bucket(&self, bucket_name: &str) -> Result<Bucket, ObjectStorageServiceError> {
        let gcp_bucket: GcpBucket = block_on(self.client.get_bucket(&GetBucketRequest {
            bucket: bucket_name.to_string(),
            if_metageneration_match: None,
            if_metageneration_not_match: None,
            projection: None,
        }))
        .map_err(|e| ObjectStorageServiceError::CannotGetBucket {
            bucket_name: bucket_name.to_string(),
            raw_error_message: e.to_string(),
        })?;

        Bucket::try_from(gcp_bucket).map_err(|e| ObjectStorageServiceError::CannotGetBucket {
            // TODO(ENG-1813): introduce dedicated conversion error for bucket
            bucket_name: bucket_name.to_string(),
            raw_error_message: e.to_string(),
        })
    }

    pub fn create_bucket(
        &self,
        project_id: &str,
        bucket_name: &str,
        bucket_location: GcpStorageRegion,
        bucket_ttl: Option<Duration>,
        bucket_versioning_activated: bool,
        bucket_labels: Option<HashMap<String, String>>,
    ) -> Result<Bucket, ObjectStorageServiceError> {
        // Minimal TTL is 1 day for Google storage
        let bucket_ttl = bucket_ttl.map(|ttl| max(ttl, Duration::from_secs(60 * 60 * 24)));

        let mut create_bucket_request = InsertBucketRequest {
            name: bucket_name.to_string(),
            param: InsertBucketParam {
                project: project_id.to_string(),
                ..Default::default()
            },
            bucket: BucketCreationConfig {
                labels: bucket_labels,
                location: bucket_location.to_cloud_provider_format().to_uppercase(),
                versioning: match bucket_versioning_activated {
                    false => None,
                    true => Some(Versioning { enabled: true }),
                },
                ..Default::default()
            },
        };

        if let Some(ttl) = bucket_ttl {
            let bucket_ttl_max_age = i32::try_from(ttl.as_secs() / 60 / 60 / 24).map_err(|_e| {
                ObjectStorageServiceError::CannotCreateBucket {
                    bucket_name: bucket_name.to_string(),
                    raw_error_message: format!(
                        "Cannot convert bucket TTL value `{}` to fit i32 as required by Google API",
                        ttl.as_secs() / 60 / 60 / 24
                    ),
                }
            })?;
            create_bucket_request.bucket.lifecycle = Some(Lifecycle {
                rule: vec![Rule {
                    action: Some(Action {
                        r#type: ActionType::Delete,
                        storage_class: None,
                    }),
                    condition: Some(Condition {
                        age: bucket_ttl_max_age,
                        ..Default::default()
                    }),
                }],
            });
        }

        self.wait_for_a_slot_in_admission_control(Duration::from_secs(10 * 60), StorageResourceKind::Bucket)?;
        match block_on(self.client.insert_bucket(&create_bucket_request)) {
            Ok(created_bucket) => {
                Bucket::try_from(created_bucket).map_err(|e| ObjectStorageServiceError::CannotCreateBucket {
                    bucket_name: bucket_name.to_string(),
                    raw_error_message: e.to_string(),
                })
            }
            Err(e) => Err(ObjectStorageServiceError::CannotCreateBucket {
                bucket_name: bucket_name.to_string(),
                raw_error_message: e.to_string(),
            }),
        }
    }

    pub fn update_bucket(
        &self,
        bucket_name: &str,
        bucket_versioning_activated: bool,
    ) -> Result<Bucket, ObjectStorageServiceError> {
        let patch_bucket_request = PatchBucketRequest {
            bucket: bucket_name.to_string(),
            metadata: Some(BucketPatchConfig {
                versioning: Some(Versioning {
                    enabled: bucket_versioning_activated,
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        self.wait_for_a_slot_in_admission_control(Duration::from_secs(10 * 60), StorageResourceKind::Bucket)?;
        match block_on(self.client.patch_bucket(&patch_bucket_request)) {
            Ok(updated_bucket) => {
                Bucket::try_from(updated_bucket).map_err(|e| ObjectStorageServiceError::CannotUpdateBucket {
                    bucket_name: bucket_name.to_string(),
                    raw_error_message: e.to_string(),
                })
            }
            Err(e) => Err(ObjectStorageServiceError::CannotUpdateBucket {
                bucket_name: bucket_name.to_string(),
                raw_error_message: e.to_string(),
            }),
        }
    }

    pub fn delete_bucket(
        &self,
        bucket_name: &str,
        force_delete_objects: bool,
    ) -> Result<(), ObjectStorageServiceError> {
        if force_delete_objects {
            self.empty_bucket(bucket_name)?;
        }

        self.wait_for_a_slot_in_admission_control(Duration::from_secs(10 * 60), StorageResourceKind::Bucket)?;
        block_on(self.client.delete_bucket(&DeleteBucketRequest {
            bucket: bucket_name.to_string(),
            param: Default::default(),
        }))
        .map_err(|e| ObjectStorageServiceError::CannotDeleteBucket {
            bucket_name: bucket_name.to_string(),
            raw_error_message: e.to_string(),
        })
    }

    pub fn delete_object(&self, bucket_name: &str, object_id: &str) -> Result<(), ObjectStorageServiceError> {
        self.wait_for_a_slot_in_admission_control(Duration::from_secs(10 * 60), StorageResourceKind::Object)?;
        block_on(self.client.delete_object(&DeleteObjectRequest {
            bucket: bucket_name.to_string(),
            object: object_id.to_string(),
            ..Default::default()
        }))
        .map_err(|e| ObjectStorageServiceError::CannotDeleteObject {
            bucket_name: bucket_name.to_string(),
            object_id: object_id.to_string(),
            raw_error_message: e.to_string(),
        })
    }

    /// This to handle buckets having big / lot of objects deletion as it can be very long and might lead to timeout
    /// On cluster deletion, a job is created on Google side to handle the bucket deletion asynchronously.
    /// Note: there is no way as of today to setup a lifetime on bucket to set a TTL, hence need to handle it manually.
    pub fn delete_bucket_non_blocking(
        &self,
        bucket_name: &str,
        bucket_location: GcpStorageRegion,
    ) -> Result<(), ObjectStorageServiceError> {
        match self.cloud_job_service.create_job(
            format!("delete-bucket-{}", bucket_name).as_str(),
            "gcr.io/google.com/cloudsdktool/google-cloud-cli:latest",
            "gcloud",
            &[
                "storage",
                "rm",
                "--recursive",
                format!("gs://{bucket_name}", bucket_name = bucket_name).as_str(),
            ],
            self.client_email.as_str(),
            self.project_id.as_str(),
            GcpCloudJobRegion::from_str(bucket_location.to_cloud_provider_format()).map_err(|_| {
                ObjectStorageServiceError::CannotDeleteBucket {
                    bucket_name: bucket_name.to_string(),
                    raw_error_message: format!(
                        "Cannot create run job to delete the bucket, invalid region: {}",
                        bucket_location.to_cloud_provider_format()
                    ),
                }
            })?,
            true,
            Some(HashMap::from([
                // Tags keys rule: Only hyphens (-), underscores (_), lowercase characters, and numbers are allowed.
                // Keys must start with a lowercase character. International characters are allowed.
                ("action".to_string(), "bucket-deletion-async".to_string()),
                ("bucket_name".to_string(), bucket_name.to_string()),
                (
                    "bucket_location".to_string(),
                    bucket_location.to_cloud_provider_format().to_lowercase(),
                ),
            ])),
        ) {
            Ok(_) => Ok(()),
            Err(e) => Err(ObjectStorageServiceError::CannotDeleteBucket {
                bucket_name: bucket_name.to_string(),
                raw_error_message: format!("Cannot create run job to delete the bucket: {e}"),
            }),
        }
    }

    pub fn empty_bucket(&self, bucket_name: &str) -> Result<(), ObjectStorageServiceError> {
        let objects: Vec<BucketObject> = self.list_objects(bucket_name, None)?;
        for object in objects {
            self.wait_for_a_slot_in_admission_control(Duration::from_secs(10 * 60), StorageResourceKind::Object)?;
            self.delete_object(bucket_name, object.key.as_str())?;
        }

        Ok(())
    }

    pub fn list_buckets(
        &self,
        project_id: &str,
        bucket_name_prefix: Option<&str>,
    ) -> Result<Vec<Bucket>, ObjectStorageServiceError> {
        let mut buckets: Vec<Bucket> = vec![];
        let mut next_page_token: Option<String> = None;

        loop {
            match block_on(self.client.list_buckets(&ListBucketsRequest {
                project: project_id.to_string(),
                page_token: next_page_token,
                prefix: bucket_name_prefix.map(str::to_string),
                max_results: Some(1000),
                ..Default::default()
            })) {
                Ok(buckets_list_response) => {
                    next_page_token = buckets_list_response.next_page_token;
                    for gcp_bucket in buckets_list_response.items {
                        buckets.push(Bucket::try_from(gcp_bucket).map_err(|e| {
                            ObjectStorageServiceError::CannotListBuckets {
                                // TODO(ENG-1813): introduce dedicated conversion error for bucket
                                raw_error_message: e.to_string(),
                            }
                        })?)
                    }

                    if next_page_token.is_none() {
                        break;
                    }
                }
                Err(e) => {
                    return Err(ObjectStorageServiceError::CannotListBuckets {
                        raw_error_message: e.to_string(),
                    })
                }
            }
        }

        Ok(buckets)
    }

    pub fn put_object(
        &self,
        bucket_name: &str,
        object_key: &str,
        content: Vec<u8>,
    ) -> Result<BucketObject, ObjectStorageServiceError> {
        self.wait_for_a_slot_in_admission_control(Duration::from_secs(10 * 60), StorageResourceKind::Object)?;
        match block_on(self.client.upload_object(
            &UploadObjectRequest {
                bucket: bucket_name.to_string(),
                ..Default::default()
            },
            Body::from(content),
            &UploadType::Multipart(Box::new(GcpObject {
                name: object_key.to_string(),
                ..Default::default()
            })),
        )) {
            Ok(o) => self.get_object(bucket_name, o.name.as_str()),
            Err(e) => Err(ObjectStorageServiceError::CannotPutObjectToBucket {
                bucket_name: bucket_name.to_string(),
                object_key: object_key.to_string(),
                raw_error_message: e.to_string(),
            }),
        }
    }

    pub fn get_object(&self, bucket_name: &str, object_key: &str) -> Result<BucketObject, ObjectStorageServiceError> {
        let object_request = GetObjectRequest {
            bucket: bucket_name.to_string(),
            object: object_key.to_string(),
            ..Default::default()
        };
        let object = block_on(self.client.get_object(&object_request)).map_err(|e| {
            ObjectStorageServiceError::CannotGetObject {
                bucket_name: bucket_name.to_string(),
                object_key: object_key.to_string(),
                raw_error_message: e.to_string(),
            }
        })?;
        let object_content =
            block_on(self.client.download_object(&object_request, &Range(None, None))).map_err(|e| {
                ObjectStorageServiceError::CannotGetObject {
                    bucket_name: bucket_name.to_string(),
                    object_key: object_key.to_string(),
                    raw_error_message: e.to_string(),
                }
            })?;

        Ok(BucketObject {
            bucket_name: object.bucket.to_string(),
            key: object.name,
            value: object_content,
            tags: vec![],
        })
    }

    pub fn list_objects_keys_only(
        &self,
        bucket_name: &str,
        object_id_prefix: Option<&str>,
    ) -> Result<Vec<String>, ObjectStorageServiceError> {
        let mut objects: Vec<String> = vec![];
        let mut next_page_token: Option<String> = None;

        loop {
            match block_on(self.client.list_objects(&ListObjectsRequest {
                page_token: next_page_token,
                bucket: bucket_name.to_string(),
                prefix: object_id_prefix.map(str::to_string),
                max_results: Some(1000),
                ..Default::default()
            })) {
                Ok(objects_list_response) => {
                    next_page_token = objects_list_response.next_page_token;
                    if let Some(new_objects) = objects_list_response.items {
                        objects.extend(new_objects.iter().map(|o| o.name.to_string()));
                    }

                    if next_page_token.is_none() {
                        break;
                    }
                }
                Err(e) => {
                    return Err(ObjectStorageServiceError::CannotListObjects {
                        bucket_name: bucket_name.to_string(),
                        raw_error_message: e.to_string(),
                    })
                }
            }
        }

        Ok(objects)
    }

    /// List all objects with given predicates.
    /// This function should be used wisely has a GET request is triggered per object.
    pub fn list_objects(
        &self,
        bucket_name: &str,
        object_id_prefix: Option<&str>,
    ) -> Result<Vec<BucketObject>, ObjectStorageServiceError> {
        let mut objects: Vec<BucketObject> = vec![];
        let mut next_page_token: Option<String> = None;

        loop {
            match block_on(self.client.list_objects(&ListObjectsRequest {
                page_token: next_page_token,
                bucket: bucket_name.to_string(),
                prefix: object_id_prefix.map(str::to_string),
                max_results: Some(1000),
                ..Default::default()
            })) {
                Ok(objects_list_response) => {
                    next_page_token = objects_list_response.next_page_token;

                    if let Some(fetched_objects) = objects_list_response.items {
                        for object in fetched_objects {
                            objects.push(self.get_object(object.bucket.as_str(), object.name.as_str())?);
                        }
                    }

                    if next_page_token.is_none() {
                        break;
                    }
                }
                Err(e) => {
                    return Err(ObjectStorageServiceError::CannotListObjects {
                        bucket_name: bucket_name.to_string(),
                        raw_error_message: e.to_string(),
                    })
                }
            }
        }

        Ok(objects)
    }
}

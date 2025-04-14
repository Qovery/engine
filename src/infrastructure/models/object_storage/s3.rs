use chrono::{DateTime, Utc};
use itertools::Itertools;
use retry::delay::Fixed;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use crate::infrastructure::models::cloud_provider::aws::regions::AwsRegion;
use rusoto_core::{Client, HttpClient, Region as RusotoRegion};
use rusoto_s3::{
    BucketLoggingStatus, CreateBucketConfiguration, CreateBucketRequest, Delete, DeleteBucketRequest,
    DeleteObjectRequest, DeleteObjectsRequest, GetBucketLifecycleRequest, GetBucketLoggingRequest,
    GetBucketTaggingRequest, GetBucketVersioningRequest, GetObjectRequest, GetObjectTaggingRequest, HeadBucketRequest,
    ListObjectsRequest, LoggingEnabled, ObjectIdentifier, PutBucketLoggingRequest, PutBucketTaggingRequest,
    PutBucketVersioningRequest, PutObjectRequest, S3 as RusotoS3, S3Client, StreamingBody, Tag, Tagging,
};

use crate::environment::models::ToCloudProviderFormat;
use crate::infrastructure::models::cloud_provider::aws::{AwsCredentials, new_rusoto_creds};
use crate::infrastructure::models::object_storage::errors::ObjectStorageError;
use crate::infrastructure::models::object_storage::{
    Bucket, BucketDeleteStrategy, BucketObject, BucketRegion, Kind, ObjectStorage,
};
use crate::runtime::block_on;

pub struct S3 {
    id: String,
    name: String,
    credentials: AwsCredentials,
    region: AwsRegion,
}

impl S3 {
    pub fn new(id: String, name: String, credentials: AwsCredentials, region: AwsRegion) -> Self {
        S3 {
            id,
            name,
            credentials,
            region,
        }
    }

    fn get_s3_client(&self) -> S3Client {
        let region = RusotoRegion::from_str(self.region.to_cloud_provider_format()).unwrap_or_else(|_| {
            panic!(
                "S3 region `{}` doesn't seems to be valid.",
                self.region.to_cloud_provider_format()
            )
        });
        let client = Client::new_with(
            new_rusoto_creds(&self.credentials),
            HttpClient::new().expect("unable to create new Http client"),
        );

        S3Client::new_with_client(client, region)
    }

    fn is_bucket_name_valid(bucket_name: &str) -> Result<(), ObjectStorageError> {
        if bucket_name.is_empty() {
            return Err(ObjectStorageError::InvalidBucketName {
                bucket_name: bucket_name.to_string(),
                raw_error_message: "bucket name cannot be empty".to_string(),
            });
        }

        Ok(())
    }

    fn empty_bucket(&self, bucket_name: &str) -> Result<(), ObjectStorageError> {
        S3::is_bucket_name_valid(bucket_name)?;

        let s3_client = self.get_s3_client();

        // make sure to delete all bucket content before trying to delete the bucket
        let objects_to_be_deleted = match block_on(s3_client.list_objects(ListObjectsRequest {
            bucket: bucket_name.to_string(),
            ..Default::default()
        })) {
            Ok(res) => res.contents.unwrap_or_default(),
            Err(_) => {
                vec![]
            }
        };

        if !objects_to_be_deleted.is_empty() {
            if let Err(e) = block_on(
                s3_client.delete_objects(DeleteObjectsRequest {
                    bucket: bucket_name.to_string(),
                    delete: Delete {
                        objects: objects_to_be_deleted
                            .iter()
                            .filter_map(|e| e.key.clone())
                            .map(|e| ObjectIdentifier {
                                key: e,
                                version_id: None,
                            })
                            .collect(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
            ) {
                return Err(ObjectStorageError::CannotEmptyBucket {
                    bucket_name: bucket_name.to_string(),
                    raw_error_message: e.to_string(),
                });
            }
        }

        Ok(())
    }

    fn get_tags(&self, bucket_name: &str, object_key: &str) -> Vec<String> {
        let s3_client = self.get_s3_client();

        match block_on(s3_client.get_object_tagging(GetObjectTaggingRequest {
            bucket: bucket_name.to_string(),
            key: object_key.to_string(),
            expected_bucket_owner: None,
            ..Default::default()
        })) {
            Ok(res) => res
                .tag_set
                .iter()
                .map(|tag| format!("{}={}", tag.key, tag.value))
                .collect_vec(),
            Err(_) => vec![],
        }
    }
}

impl ObjectStorage for S3 {
    fn kind(&self) -> Kind {
        Kind::S3
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn is_valid(&self) -> Result<(), ObjectStorageError> {
        // TODO check valid credentials
        Ok(())
    }

    fn bucket_exists(&self, bucket_name: &str) -> bool {
        let s3_client = self.get_s3_client();

        // Note: Using rusoto here for convenience, should be possible via CLI but would be way less convenient.
        // Using retry here since there is a lag after bucket creation
        retry::retry(Fixed::from_millis(1000).take(10), || {
            block_on(s3_client.head_bucket(HeadBucketRequest {
                bucket: bucket_name.to_string(),
                expected_bucket_owner: None,
            }))
        })
        .is_ok()
    }

    fn create_bucket(
        &self,
        bucket_name: &str,
        bucket_ttl: Option<Duration>,
        bucket_versioning_activated: bool,
        bucket_logging_activated: bool,
    ) -> Result<Bucket, ObjectStorageError> {
        S3::is_bucket_name_valid(bucket_name)?;

        let s3_client = self.get_s3_client();

        // check if bucket already exists, if so, no need to recreate it
        if let Ok(existing_bucket) = self.get_bucket(bucket_name) {
            return Ok(existing_bucket);
        }

        if let Err(e) = block_on(s3_client.create_bucket(CreateBucketRequest {
            bucket: bucket_name.to_string(),
            create_bucket_configuration: Some(CreateBucketConfiguration {
                location_constraint: Some(self.region.to_cloud_provider_format().to_string()),
            }),
            ..Default::default()
        })) {
            return Err(ObjectStorageError::CannotCreateBucket {
                bucket_name: bucket_name.to_string(),
                raw_error_message: e.to_string(),
            });
        }

        let creation_date: DateTime<Utc> = Utc::now();
        if let Err(e) = block_on(s3_client.put_bucket_tagging(PutBucketTaggingRequest {
            bucket: bucket_name.to_string(),
            expected_bucket_owner: None,
            tagging: Tagging {
                tag_set: vec![
                    Tag {
                        key: "CreationDate".to_string(),
                        value: creation_date.to_rfc3339(),
                    },
                    Tag {
                        key: "Ttl".to_string(),
                        value: format!("{}", bucket_ttl.map(|ttl| ttl.as_secs()).unwrap_or(0)),
                    },
                ],
            },
            ..Default::default()
        })) {
            return Err(ObjectStorageError::CannotTagBucket {
                bucket_name: bucket_name.to_string(),
                raw_error_message: e.to_string(),
            });
        }

        if bucket_versioning_activated {
            // Not blocking if fails for the time being
            match block_on(s3_client.put_bucket_versioning(PutBucketVersioningRequest {
                bucket: bucket_name.to_string(),
                expected_bucket_owner: None,
                ..Default::default()
            })) {
                Ok(_) => {}
                Err(e) => {
                    return Err(ObjectStorageError::CannotActivateBucketVersioning {
                        bucket_name: bucket_name.to_string(),
                        raw_error_message: e.to_string(),
                    });
                }
            }
        }

        if bucket_logging_activated {
            // Not blocking if fails for the time being
            match block_on(s3_client.put_bucket_logging(PutBucketLoggingRequest {
                bucket: bucket_name.to_string(),
                bucket_logging_status: BucketLoggingStatus {
                    logging_enabled: Some(LoggingEnabled {
                        target_bucket: bucket_name.to_string(),
                        target_grants: None,
                        target_prefix: "logs/".to_string(),
                    }),
                },
                expected_bucket_owner: None,
                ..Default::default()
            })) {
                Ok(_) => {}
                Err(e) => {
                    return Err(ObjectStorageError::CannotActivateBucketLogging {
                        bucket_name: bucket_name.to_string(),
                        raw_error_message: e.to_string(),
                    });
                }
            }
        }

        self.get_bucket(bucket_name) // TODO(benjaminch): maybe doing a get here is avoidable
    }

    fn update_bucket(
        &self,
        _bucket_name: &str,
        _bucket_ttl: Option<Duration>,
        _bucket_versioning_activated: bool,
        _bucket_logging_activated: bool,
        _bucket_labels: Option<HashMap<String, String>>,
    ) -> Result<Bucket, ObjectStorageError> {
        // TODO(benjaminch): to be implemented
        todo!("update_bucket for S3 is not implemented")
    }

    fn get_bucket(&self, bucket_name: &str) -> Result<Bucket, ObjectStorageError> {
        // if bucket doesn't exist, then return an error
        if !self.bucket_exists(bucket_name) {
            return Err(ObjectStorageError::CannotGetBucket {
                bucket_name: bucket_name.to_string(),
                raw_error_message: format!("Bucket `{}` doesn't exist", bucket_name),
            });
        }

        // Get TTL
        let mut ttl: Option<Duration> = None;
        if let Ok(bl) = block_on(self.get_s3_client().get_bucket_lifecycle(GetBucketLifecycleRequest {
            bucket: bucket_name.to_string(),
            expected_bucket_owner: None,
        })) {
            if let Some(rules) = bl.rules {
                for r in rules {
                    if let Some(expiration) = r.expiration {
                        ttl = expiration
                            .days
                            .map(|days| Duration::from_secs(days.unsigned_abs() * 24 * 60 * 60));
                    }
                }
            }
        }

        // Get versioning
        let mut versioning_activated = false;
        if let Ok(versioning) = block_on(self.get_s3_client().get_bucket_versioning(GetBucketVersioningRequest {
            bucket: bucket_name.to_string(),
            expected_bucket_owner: None,
        })) {
            if let Some(status) = versioning.status.map(|s| s.to_lowercase()) {
                if status == "enabled" {
                    versioning_activated = true;
                }
            }
        }

        // Get logging
        let mut logging_activated = false;
        if let Ok(logging) = block_on(self.get_s3_client().get_bucket_logging(GetBucketLoggingRequest {
            bucket: bucket_name.to_string(),
            expected_bucket_owner: None,
        })) {
            if let Some(logging_enabled) = logging.logging_enabled {
                if logging_enabled.target_bucket == bucket_name {
                    logging_activated = true;
                }
            }
        }

        // Get labels
        let mut labels: Option<HashMap<String, String>> = None;
        if let Ok(tagging) = block_on(self.get_s3_client().get_bucket_tagging(GetBucketTaggingRequest {
            bucket: bucket_name.to_string(),
            expected_bucket_owner: None,
        })) {
            labels = Some(HashMap::from_iter(tagging.tag_set.into_iter().map(|t| (t.key, t.value))));
        }

        Ok(Bucket {
            name: bucket_name.to_string(),
            ttl,
            versioning_activated,
            logging_activated,
            location: BucketRegion::AwsRegion(self.region.clone()),
            labels,
        })
    }

    fn delete_bucket(
        &self,
        bucket_name: &str,
        bucket_delete_strategy: BucketDeleteStrategy,
    ) -> Result<(), ObjectStorageError> {
        S3::is_bucket_name_valid(bucket_name)?;

        let s3_client = self.get_s3_client();

        // make sure to delete all bucket content before trying to delete the bucket
        self.empty_bucket(bucket_name)?;

        match bucket_delete_strategy {
            BucketDeleteStrategy::HardDelete => match block_on(s3_client.delete_bucket(DeleteBucketRequest {
                bucket: bucket_name.to_string(),
                expected_bucket_owner: None,
            })) {
                Ok(_) => Ok(()),
                Err(e) => Err(ObjectStorageError::CannotDeleteBucket {
                    bucket_name: bucket_name.to_string(),
                    raw_error_message: e.to_string(),
                }),
            },
            BucketDeleteStrategy::Empty => Ok(()),
        }
    }

    fn delete_bucket_non_blocking(&self, _bucket_name: &str) -> Result<(), ObjectStorageError> {
        todo!("delete_bucket_non_blocking for S3 is not implemented")
    }

    fn get_object(&self, bucket_name: &str, object_key: &str) -> Result<BucketObject, ObjectStorageError> {
        S3::is_bucket_name_valid(bucket_name)?;

        let s3_client = self.get_s3_client();

        match block_on(s3_client.get_object(GetObjectRequest {
            bucket: bucket_name.to_string(),
            key: object_key.to_string(),
            expected_bucket_owner: None,
            ..Default::default()
        })) {
            Ok(res) => {
                let mut stream = match res.body {
                    Some(b) => b.into_blocking_read(),
                    None => {
                        return Err(ObjectStorageError::CannotGetObjectFile {
                            bucket_name: bucket_name.to_string(),
                            object_name: object_key.to_string(),
                            raw_error_message: "Cannot get response body".to_string(),
                        });
                    }
                };
                let mut body = Vec::new();
                stream
                    .read_to_end(&mut body)
                    .map_err(|e| ObjectStorageError::CannotGetObjectFile {
                        bucket_name: bucket_name.to_string(),
                        object_name: object_key.to_string(),
                        raw_error_message: format!("Cannot read response body: {}", e).to_string(),
                    })?;

                let tags = match res.tag_count {
                    Some(tag_count) if tag_count > 0 => self.get_tags(bucket_name, object_key),
                    _ => vec![],
                };

                Ok(BucketObject {
                    bucket_name: bucket_name.to_string(),
                    key: object_key.to_string(),
                    value: body,
                    tags,
                })
            }
            Err(e) => Err(ObjectStorageError::CannotGetObjectFile {
                bucket_name: bucket_name.to_string(),
                object_name: object_key.to_string(),
                raw_error_message: e.to_string(),
            }),
        }
    }

    fn put_object(
        &self,
        bucket_name: &str,
        object_key: &str,
        file_path: &Path,
        tags: Option<Vec<String>>,
    ) -> Result<BucketObject, ObjectStorageError> {
        S3::is_bucket_name_valid(bucket_name)?;

        let s3_client = self.get_s3_client();

        let file_content = std::fs::read(file_path).map_err(|e| ObjectStorageError::CannotUploadFile {
            bucket_name: bucket_name.to_string(),
            object_name: object_key.to_string(),
            raw_error_message: e.to_string(),
        })?;

        let tags = tags.map(|tags| tags.join("&"));

        match block_on(s3_client.put_object(PutObjectRequest {
            bucket: bucket_name.to_string(),
            key: object_key.to_string(),
            body: Some(StreamingBody::from(file_content.clone())),
            expected_bucket_owner: None,
            tagging: tags,
            ..Default::default()
        })) {
            Ok(_o) => Ok(BucketObject {
                bucket_name: bucket_name.to_string(),
                key: object_key.to_string(),
                value: file_content.clone(),
                tags: vec![],
            }),
            Err(e) => Err(ObjectStorageError::CannotUploadFile {
                bucket_name: bucket_name.to_string(),
                object_name: object_key.to_string(),
                raw_error_message: e.to_string(),
            }),
        }
    }

    fn delete_object(&self, bucket_name: &str, object_key: &str) -> Result<(), ObjectStorageError> {
        if S3::is_bucket_name_valid(bucket_name).is_err() {
            // bucket is missing it's ok as file can't be present
            return Ok(());
        };

        // check if file already exists
        if self.get_object(bucket_name, object_key).is_err() {
            return Ok(());
        };

        let s3_client = self.get_s3_client();

        match block_on(s3_client.delete_object(DeleteObjectRequest {
            bucket: bucket_name.to_string(),
            key: object_key.to_string(),
            ..Default::default()
        })) {
            Ok(_) => Ok(()),
            Err(e) => Err(ObjectStorageError::CannotDeleteFile {
                bucket_name: bucket_name.to_string(),
                object_name: object_key.to_string(),
                raw_error_message: e.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestCase<'a> {
        bucket_name_input: &'a str,
        expected_output: Result<(), ObjectStorageError>,
        description: &'a str,
    }

    #[test]
    fn test_is_bucket_name_valid() {
        // setup:
        let test_cases: Vec<TestCase> = vec![
            TestCase {
                bucket_name_input: "",
                expected_output: Err(ObjectStorageError::InvalidBucketName {
                    bucket_name: "".to_string(),
                    raw_error_message: "bucket name cannot be empty".to_string(),
                }),
                description: "bucket name is empty",
            },
            TestCase {
                bucket_name_input: "valid",
                expected_output: Ok(()),
                description: "bucket name is valid",
            },
        ];

        for tc in test_cases {
            // execute:
            let result = S3::is_bucket_name_valid(tc.bucket_name_input);

            // verify:
            assert_eq!(tc.expected_output, result, "{}", tc.description);
        }
    }
}

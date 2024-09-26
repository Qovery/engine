use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use crate::object_storage::{Bucket, BucketDeleteStrategy, BucketObject, BucketRegion, Kind, ObjectStorage};

use crate::models::scaleway::ScwZone;
use crate::object_storage::errors::ObjectStorageError;
use crate::runtime::block_on;
use rusoto_core::{Client, HttpClient, Region as RusotoRegion};
use rusoto_credential::StaticProvider;
use rusoto_s3::{
    CreateBucketConfiguration, CreateBucketRequest, Delete, DeleteBucketRequest, DeleteObjectRequest,
    DeleteObjectsRequest, GetBucketLifecycleRequest, GetBucketTaggingRequest, GetBucketVersioningRequest,
    GetObjectRequest, HeadBucketRequest, ListObjectsRequest, ObjectIdentifier, PutBucketTaggingRequest,
    PutBucketVersioningRequest, PutObjectRequest, S3Client, StreamingBody, Tag, Tagging, S3,
};

// doc: https://www.scaleway.com/en/docs/object-storage-feature/
pub struct ScalewayOS {
    id: String,
    name: String,
    access_key: String,
    secret_token: String,
    zone: ScwZone,
}

impl ScalewayOS {
    pub fn new(id: String, name: String, access_key: String, secret_token: String, zone: ScwZone) -> ScalewayOS {
        ScalewayOS {
            id,
            name,
            access_key,
            secret_token,
            zone,
        }
    }

    fn get_s3_client(&self) -> S3Client {
        let region = RusotoRegion::Custom {
            name: self.zone.region().to_string(),
            endpoint: self.get_endpoint_url_for_region(),
        };

        let client = Client::new_with(self.get_credentials(), HttpClient::new().unwrap());

        S3Client::new_with_client(client, region)
    }

    fn get_credentials(&self) -> StaticProvider {
        StaticProvider::new(self.access_key.clone(), self.secret_token.clone(), None, None)
    }

    fn get_endpoint_url_for_region(&self) -> String {
        format!("https://s3.{}.scw.cloud", self.zone.region())
    }

    fn is_bucket_name_valid(bucket_name: &str) -> Result<(), ObjectStorageError> {
        if bucket_name.is_empty() {
            return Err(ObjectStorageError::InvalidBucketName {
                bucket_name: bucket_name.to_string(),
                raw_error_message: "bucket name cannot be empty".to_string(),
            });
        }
        // From Scaleway doc
        // Note: The SSL certificate does not support bucket names containing additional dots (.).
        // You may receive a SSL warning in your browser when accessing a bucket like my.bucket.name.s3.fr-par.scw.cloud
        // and it is recommended to use dashes (-) instead: my-bucket-name.s3.fr-par.scw.cloud.
        if bucket_name.contains('.') {
            return Err(ObjectStorageError::InvalidBucketName {
                bucket_name: bucket_name.to_string(),
                raw_error_message: "bucket name cannot contain '.' in its name, recommended to use '-' instead"
                    .to_string(),
            });
        }

        Ok(())
    }

    fn empty_bucket(&self, bucket_name: &str) -> Result<(), ObjectStorageError> {
        // TODO(benjamin): switch to `scaleway-api-rs` once object storage will be supported (https://github.com/Qovery/scaleway-api-rs/issues/12).
        ScalewayOS::is_bucket_name_valid(bucket_name)?;

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
}

impl ObjectStorage for ScalewayOS {
    fn kind(&self) -> Kind {
        Kind::ScalewayOs
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn is_valid(&self) -> Result<(), ObjectStorageError> {
        Ok(())
    }

    fn bucket_exists(&self, bucket_name: &str) -> bool {
        let s3_client = self.get_s3_client();

        block_on(s3_client.head_bucket(HeadBucketRequest {
            bucket: bucket_name.to_string(),
            expected_bucket_owner: None,
        }))
        .is_ok()
    }

    fn create_bucket(
        &self,
        bucket_name: &str,
        bucket_ttl: Option<Duration>,
        bucket_versioning_activated: bool,
    ) -> Result<Bucket, ObjectStorageError> {
        // TODO(benjamin): switch to `scaleway-api-rs` once object storage will be supported (https://github.com/Qovery/scaleway-api-rs/issues/12).
        ScalewayOS::is_bucket_name_valid(bucket_name)?;

        let s3_client = self.get_s3_client();

        // check if bucket already exists, if so, no need to recreate it
        // note: we are not deleting buckets since it takes up to 24 hours to be taken into account
        // so we better reuse existing ones
        if let Ok(existing_bucket) = self.get_bucket(bucket_name) {
            return Ok(existing_bucket);
        }

        if let Err(e) = block_on(s3_client.create_bucket(CreateBucketRequest {
            bucket: bucket_name.to_string(),
            create_bucket_configuration: Some(CreateBucketConfiguration {
                location_constraint: Some(self.zone.region().to_string()),
            }),
            ..Default::default()
        })) {
            return Err(ScalewayObjectStorageErrorManager::try_extract_fully_qualified_error(
                e.to_string().as_str(),
                Some(bucket_name),
            )
            .unwrap_or_else(|| ObjectStorageError::CannotCreateBucket {
                bucket_name: bucket_name.to_string(),
                raw_error_message: e.to_string(),
            }));
        }

        let creation_date: DateTime<Utc> = Utc::now();
        if let Err(e) = block_on(s3_client.put_bucket_tagging(PutBucketTaggingRequest {
            bucket: bucket_name.to_string(),
            expected_bucket_owner: None,
            // Note: SCW doesn't support key/value tags, keys should be added inside the value
            tagging: Tagging {
                tag_set: vec![
                    Tag {
                        key: "CreationDate".to_string(),
                        value: format!("CreationDate:{}", creation_date.to_rfc3339()),
                    },
                    Tag {
                        key: "Ttl".to_string(),
                        value: format!("Ttl:{}", bucket_ttl.map(|ttl| ttl.as_secs()).unwrap_or(0)),
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
            if let Err(_e) = block_on(s3_client.put_bucket_versioning(PutBucketVersioningRequest {
                bucket: bucket_name.to_string(),
                ..Default::default()
            })) {
                // TODO(benjaminch): to be investigated, versioning seems to fail
                // Not blocking if it fails
                // Err(self.engine_error(ObjectStorageErrorCause::Internal, message))
            }
        }

        self.get_bucket(bucket_name) // TODO(benjaminch): maybe doing a get here is avoidable
    }

    fn update_bucket(
        &self,
        _bucket_name: &str,
        _bucket_versioning_activated: bool,
    ) -> Result<Bucket, ObjectStorageError> {
        // TODO(benjaminch): to be implemented
        todo!("update_bucket for SCW object storage is not implemented")
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
            location: BucketRegion::ScwRegion(self.zone),
            labels,
        })
    }

    fn delete_bucket(
        &self,
        bucket_name: &str,
        bucket_delete_strategy: BucketDeleteStrategy,
    ) -> Result<(), ObjectStorageError> {
        // TODO(benjamin): switch to `scaleway-api-rs` once object storage will be supported (https://github.com/Qovery/scaleway-api-rs/issues/12).
        ScalewayOS::is_bucket_name_valid(bucket_name)?;

        let s3_client = self.get_s3_client();

        // make sure to delete all bucket content before trying to delete the bucket
        self.empty_bucket(bucket_name)?;

        // Note: Do not delete the bucket entirely but empty its content.
        // Bucket deletion might take up to 24 hours and during this time we are not able to create a bucket with the same name.
        // So emptying bucket allows future reuse.
        match bucket_delete_strategy {
            BucketDeleteStrategy::HardDelete => match block_on(s3_client.delete_bucket(DeleteBucketRequest {
                bucket: bucket_name.to_string(),
                ..Default::default()
            })) {
                Ok(_) => Ok(()),
                Err(e) => Err(ObjectStorageError::CannotDeleteBucket {
                    bucket_name: bucket_name.to_string(),
                    raw_error_message: e.to_string(),
                }),
            },
            BucketDeleteStrategy::Empty => Ok(()), // Do not delete the bucket
        }
    }

    fn delete_bucket_non_blocking(&self, _bucket_name: &str) -> Result<(), ObjectStorageError> {
        todo!("delete_bucket for SCW is not implemented")
    }

    fn get_object(&self, bucket_name: &str, object_key: &str) -> Result<BucketObject, ObjectStorageError> {
        // TODO(benjamin): switch to `scaleway-api-rs` once object storage will be supported (https://github.com/Qovery/scaleway-api-rs/issues/12).
        ScalewayOS::is_bucket_name_valid(bucket_name)?;

        let s3_client = self.get_s3_client();

        match block_on(s3_client.get_object(GetObjectRequest {
            bucket: bucket_name.to_string(),
            key: object_key.to_string(),
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
                        })
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

                Ok(BucketObject {
                    bucket_name: bucket_name.to_string(),
                    key: object_key.to_string(),
                    value: body,
                    tags: vec![],
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
        _tags: Option<Vec<String>>,
    ) -> Result<BucketObject, ObjectStorageError> {
        // TODO(benjamin): switch to `scaleway-api-rs` once object storage will be supported (https://github.com/Qovery/scaleway-api-rs/issues/12).
        ScalewayOS::is_bucket_name_valid(bucket_name)?;

        let s3_client = self.get_s3_client();

        let file_content = std::fs::read(file_path).map_err(|e| ObjectStorageError::CannotUploadFile {
            bucket_name: bucket_name.to_string(),
            object_name: object_key.to_string(),
            raw_error_message: e.to_string(),
        })?;

        match block_on(s3_client.put_object(PutObjectRequest {
            bucket: bucket_name.to_string(),
            key: object_key.to_string(),
            body: Some(StreamingBody::from(file_content.clone())),
            ..Default::default()
        })) {
            Ok(_) => Ok(BucketObject {
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
        if ScalewayOS::is_bucket_name_valid(bucket_name).is_err() {
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

struct ScalewayObjectStorageErrorManager {}

impl ScalewayObjectStorageErrorManager {
    /// Try to find a more qualified specific error from an Object Storage error
    fn try_extract_fully_qualified_error(
        raw_error_message: &str,
        bucket_name: Option<&str>,
    ) -> Option<ObjectStorageError> {
        if raw_error_message.contains("<Code>QuotaExceeded</Code>") {
            return Some(ObjectStorageError::QuotasExceeded {
                raw_error_message: raw_error_message.to_string(),
                bucket_name: bucket_name.unwrap_or_default().to_string(),
            });
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_bucket_name_valid() {
        struct TestCase<'a> {
            bucket_name_input: &'a str,
            expected_output: Result<(), ObjectStorageError>,
            description: &'a str,
        }

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
                bucket_name_input: "containing.dot",
                expected_output: Err(ObjectStorageError::InvalidBucketName {
                    bucket_name: "containing.dot".to_string(),
                    raw_error_message: "bucket name cannot contain '.' in its name, recommended to use '-' instead"
                        .to_string(),
                }),
                description: "bucket name contains dot char",
            },
            TestCase {
                bucket_name_input: "valid",
                expected_output: Ok(()),
                description: "bucket name is valid",
            },
        ];

        for tc in test_cases {
            // execute:
            let result = ScalewayOS::is_bucket_name_valid(tc.bucket_name_input);

            // verify:
            assert_eq!(tc.expected_output, result, "{}", tc.description);
        }
    }

    #[test]
    fn test_object_storage_error_manager_quota_exceeded() {
        // setup:
        struct TestCase<'a> {
            raw_error_message: &'a str,
            bucket_name: Option<&'a str>,
            expected_output: Option<ObjectStorageError>,
            description: &'a str,
        }

        let test_cases = vec![
            TestCase{
            bucket_name: Some("test-bucket"),
            raw_error_message: "Request ID: None Body: <?xml version='1.0' encoding='UTF-8'?>\n<Error><Code>QuotaExceeded</Code><Message>Quota exceeded. Please contact support to upgrade your quotas.</Message><RequestId>txbdb89084fcb04c36a2b49-0062d9937c</RequestId></Error>",
            expected_output: Some(ObjectStorageError::QuotasExceeded {
                bucket_name: "test-bucket".to_string(),
                raw_error_message: "Request ID: None Body: <?xml version='1.0' encoding='UTF-8'?>\n<Error><Code>QuotaExceeded</Code><Message>Quota exceeded. Please contact support to upgrade your quotas.</Message><RequestId>txbdb89084fcb04c36a2b49-0062d9937c</RequestId></Error>".to_string()
            }),
            description: "Case 1 - Nominal case, there is a quotas issue",
        },
                              TestCase{
                                  bucket_name: Some("test-bucket"),
                                      raw_error_message: "Request ID: None Body: <?xml version='1.0' encoding='UTF-8'?>\n<Error><Code>RandomIssue</Code><Message>Random issue message description.</Message><RequestId>txbdb89084fcb04c36a2b49-0062d9937c</RequestId></Error>",
                                  expected_output: None,
                                  description: "Case 2 - Nominal case, there is no quotas issue",
                              }];

        for tc in test_cases {
            // execute:
            let result = ScalewayObjectStorageErrorManager::try_extract_fully_qualified_error(
                tc.raw_error_message,
                tc.bucket_name,
            );

            // verify:
            assert_eq!(tc.expected_output, result, "{}", tc.description);
        }
    }
}

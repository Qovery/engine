use chrono::{DateTime, Utc};
use std::fs::File;
use std::path::Path;

use crate::io_models::domain::StringPath;
use crate::object_storage::{Kind, ObjectStorage};

use crate::io_models::context::Context;
use crate::models::scaleway::ScwZone;
use crate::object_storage::errors::ObjectStorageError;
use crate::runtime::block_on;
use rusoto_core::{Client, HttpClient, Region as RusotoRegion};
use rusoto_credential::StaticProvider;
use rusoto_s3::{
    CreateBucketConfiguration, CreateBucketRequest, Delete, DeleteBucketRequest, DeleteObjectRequest,
    DeleteObjectsRequest, GetObjectRequest, HeadBucketRequest, ListObjectsRequest, ObjectIdentifier,
    PutBucketTaggingRequest, PutBucketVersioningRequest, PutObjectRequest, S3Client, StreamingBody, Tag, Tagging, S3,
};
use tokio::io;

pub enum BucketDeleteStrategy {
    HardDelete,
    Empty,
}

// doc: https://www.scaleway.com/en/docs/object-storage-feature/
pub struct ScalewayOS {
    context: Context,
    id: String,
    name: String,
    access_key: String,
    secret_token: String,
    zone: ScwZone,
    bucket_delete_strategy: BucketDeleteStrategy,
    bucket_versioning_activated: bool,
    bucket_ttl_in_seconds: Option<u32>,
}

impl ScalewayOS {
    pub fn new(
        context: Context,
        id: String,
        name: String,
        access_key: String,
        secret_token: String,
        zone: ScwZone,
        bucket_delete_strategy: BucketDeleteStrategy,
        bucket_versioning_activated: bool,
        bucket_ttl_in_seconds: Option<u32>,
    ) -> ScalewayOS {
        ScalewayOS {
            context,
            id,
            name,
            access_key,
            secret_token,
            zone,
            bucket_delete_strategy,
            bucket_versioning_activated,
            bucket_ttl_in_seconds,
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

    pub fn bucket_exists(&self, bucket_name: &str) -> bool {
        let s3_client = self.get_s3_client();

        block_on(s3_client.head_bucket(HeadBucketRequest {
            bucket: bucket_name.to_string(),
            expected_bucket_owner: None,
        }))
        .is_ok()
    }
}

impl ObjectStorage for ScalewayOS {
    fn context(&self) -> &Context {
        &self.context
    }

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

    fn create_bucket(&self, bucket_name: &str) -> Result<(), ObjectStorageError> {
        // TODO(benjamin): switch to `scaleway-api-rs` once object storage will be supported (https://github.com/Qovery/scaleway-api-rs/issues/12).
        ScalewayOS::is_bucket_name_valid(bucket_name)?;

        let s3_client = self.get_s3_client();

        // check if bucket already exists, if so, no need to recreate it
        // note: we are not deleting buckets since it takes up to 24 hours to be taken into account
        // so we better reuse existing ones
        if self.bucket_exists(bucket_name) {
            return Ok(());
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
                        value: format!("CreationDate={}", creation_date.to_rfc3339()),
                    },
                    Tag {
                        key: "Ttl".to_string(),
                        value: format!("Ttl={}", self.bucket_ttl_in_seconds.unwrap_or(0)),
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

        if self.bucket_versioning_activated {
            if let Err(_e) = block_on(s3_client.put_bucket_versioning(PutBucketVersioningRequest {
                bucket: bucket_name.to_string(),
                ..Default::default()
            })) {
                // TODO(benjaminch): to be investigated, versioning seems to fail
                // Not blocking if it fails
                // Err(self.engine_error(ObjectStorageErrorCause::Internal, message))
            }
        }

        Ok(())
    }

    fn delete_bucket(&self, bucket_name: &str) -> Result<(), ObjectStorageError> {
        // TODO(benjamin): switch to `scaleway-api-rs` once object storage will be supported (https://github.com/Qovery/scaleway-api-rs/issues/12).
        ScalewayOS::is_bucket_name_valid(bucket_name)?;

        let s3_client = self.get_s3_client();

        // make sure to delete all bucket content before trying to delete the bucket
        if let Err(e) = self.empty_bucket(bucket_name) {
            return Err(e);
        }

        // Note: Do not delete the bucket entirely but empty its content.
        // Bucket deletion might take up to 24 hours and during this time we are not able to create a bucket with the same name.
        // So emptying bucket allows future reuse.
        match &self.bucket_delete_strategy {
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

    fn get(
        &self,
        bucket_name: &str,
        object_key: &str,
        use_cache: bool,
    ) -> Result<(StringPath, File), ObjectStorageError> {
        // TODO(benjamin): switch to `scaleway-api-rs` once object storage will be supported (https://github.com/Qovery/scaleway-api-rs/issues/12).
        ScalewayOS::is_bucket_name_valid(bucket_name)?;

        let workspace_directory = crate::fs::workspace_directory(
            self.context().workspace_root_dir(),
            self.context().execution_id(),
            format!("object-storage/scaleway_os/{}", self.name()),
        )
        .map_err(|err| ObjectStorageError::CannotGetObjectFile {
            bucket_name: bucket_name.to_string(),
            file_name: object_key.to_string(),
            raw_error_message: err.to_string(),
        })?;

        let file_path = format!("{}/{}/{}", workspace_directory, bucket_name, object_key);

        if use_cache {
            // does config file already exists?
            if let Ok(file) = File::open(file_path.as_str()) {
                return Ok((file_path, file));
            }
        }

        let s3_client = self.get_s3_client();

        match block_on(s3_client.get_object(GetObjectRequest {
            bucket: bucket_name.to_string(),
            key: object_key.to_string(),
            ..Default::default()
        })) {
            Ok(mut res) => {
                let body = res.body.take();
                let mut body = body.unwrap().into_async_read();

                // create parent dir
                let path = Path::new(file_path.as_str());
                let parent_dir = path.parent().unwrap();
                let _ = block_on(tokio::fs::create_dir_all(parent_dir));

                // create file
                match block_on(
                    tokio::fs::OpenOptions::new()
                        .create(true)
                        .write(true)
                        .truncate(true)
                        .open(path),
                ) {
                    Ok(mut created_file) => match block_on(io::copy(&mut body, &mut created_file)) {
                        Ok(_) => {
                            let file = File::open(path).unwrap();
                            Ok((file_path, file))
                        }
                        Err(e) => Err(ObjectStorageError::CannotGetObjectFile {
                            bucket_name: bucket_name.to_string(),
                            file_name: object_key.to_string(),
                            raw_error_message: e.to_string(),
                        }),
                    },
                    Err(e) => Err(ObjectStorageError::CannotGetObjectFile {
                        bucket_name: bucket_name.to_string(),
                        file_name: object_key.to_string(),
                        raw_error_message: e.to_string(),
                    }),
                }
            }
            Err(e) => Err(ObjectStorageError::CannotGetObjectFile {
                bucket_name: bucket_name.to_string(),
                file_name: object_key.to_string(),
                raw_error_message: e.to_string(),
            }),
        }
    }

    fn put(&self, bucket_name: &str, object_key: &str, file_path: &str) -> Result<(), ObjectStorageError> {
        // TODO(benjamin): switch to `scaleway-api-rs` once object storage will be supported (https://github.com/Qovery/scaleway-api-rs/issues/12).
        ScalewayOS::is_bucket_name_valid(bucket_name)?;

        let s3_client = self.get_s3_client();

        match block_on(s3_client.put_object(PutObjectRequest {
            bucket: bucket_name.to_string(),
            key: object_key.to_string(),
            body: Some(StreamingBody::from(match std::fs::read(file_path) {
                Ok(x) => x,
                Err(e) => {
                    return Err(ObjectStorageError::CannotUploadFile {
                        bucket_name: bucket_name.to_string(),
                        file_name: object_key.to_string(),
                        raw_error_message: e.to_string(),
                    })
                }
            })),
            ..Default::default()
        })) {
            Ok(_) => Ok(()),
            Err(e) => Err(ObjectStorageError::CannotUploadFile {
                bucket_name: bucket_name.to_string(),
                file_name: object_key.to_string(),
                raw_error_message: e.to_string(),
            }),
        }
    }

    fn ensure_file_is_absent(&self, bucket_name: &str, object_key: &str) -> Result<(), ObjectStorageError> {
        if ScalewayOS::is_bucket_name_valid(bucket_name).is_err() {
            // bucket is missing it's ok as file can't be present
            return Ok(());
        };

        // check if file already exists
        if self.get(bucket_name, object_key, false).is_err() {
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
                file_name: object_key.to_string(),
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

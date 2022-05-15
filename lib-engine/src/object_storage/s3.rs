use chrono::{DateTime, Utc};
use retry::delay::Fixed;
use std::fs::File;
use std::path::Path;
use std::str::FromStr;

use crate::cloud_provider::aws::regions::AwsRegion;
use rusoto_core::credential::StaticProvider;
use rusoto_core::{Client, HttpClient, Region as RusotoRegion};
use rusoto_s3::{
    CreateBucketConfiguration, CreateBucketRequest, Delete, DeleteBucketRequest, DeleteObjectsRequest,
    GetObjectRequest, HeadBucketRequest, ListObjectsRequest, ObjectIdentifier, PutBucketTaggingRequest,
    PutBucketVersioningRequest, PutObjectRequest, S3Client, StreamingBody, Tag, Tagging, S3 as RusotoS3,
};
use tokio::io;

use crate::io_models::{Context, StringPath};
use crate::object_storage::errors::ObjectStorageError;
use crate::object_storage::{Kind, ObjectStorage};
use crate::runtime::block_on;

pub struct S3 {
    context: Context,
    id: String,
    name: String,
    access_key_id: String,
    secret_access_key: String,
    region: AwsRegion,
    bucket_versioning_activated: bool,
    bucket_ttl_in_seconds: Option<u32>,
}

impl S3 {
    pub fn new(
        context: Context,
        id: String,
        name: String,
        access_key_id: String,
        secret_access_key: String,
        region: AwsRegion,
        bucket_versioning_activated: bool,
        bucket_ttl_in_seconds: Option<u32>,
    ) -> Self {
        S3 {
            context,
            id,
            name,
            access_key_id,
            secret_access_key,
            region,
            bucket_versioning_activated,
            bucket_ttl_in_seconds,
        }
    }

    fn get_credentials(&self) -> StaticProvider {
        StaticProvider::new(self.access_key_id.clone(), self.secret_access_key.clone(), None, None)
    }

    fn get_s3_client(&self) -> S3Client {
        let region = RusotoRegion::from_str(&self.region.to_aws_format())
            .unwrap_or_else(|_| panic!("S3 region `{}` doesn't seems to be valid.", self.region.to_aws_format()));
        let client = Client::new_with(
            self.get_credentials(),
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

    pub fn bucket_exists(&self, bucket_name: &str) -> bool {
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

    fn empty_bucket(&self, bucket_name: &str) -> Result<(), ObjectStorageError> {
        let _ = S3::is_bucket_name_valid(bucket_name)?;

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

impl ObjectStorage for S3 {
    fn context(&self) -> &Context {
        &self.context
    }

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

    fn create_bucket(&self, bucket_name: &str) -> Result<(), ObjectStorageError> {
        let _ = S3::is_bucket_name_valid(bucket_name)?;

        let s3_client = self.get_s3_client();

        // check if bucket already exists, if so, no need to recreate it
        if self.bucket_exists(bucket_name) {
            return Ok(());
        }

        if let Err(e) = block_on(s3_client.create_bucket(CreateBucketRequest {
            bucket: bucket_name.to_string(),
            create_bucket_configuration: Some(CreateBucketConfiguration {
                location_constraint: Some(self.region.to_aws_format()),
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
                        value: format!("{}", self.bucket_ttl_in_seconds.unwrap_or(0)),
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
            // Not blocking if fails for the ttime being
            let _ = block_on(s3_client.put_bucket_versioning(PutBucketVersioningRequest {
                bucket: bucket_name.to_string(),
                ..Default::default()
            }));
        }

        Ok(())
    }

    fn delete_bucket(&self, bucket_name: &str) -> Result<(), ObjectStorageError> {
        let _ = S3::is_bucket_name_valid(bucket_name)?;

        let s3_client = self.get_s3_client();

        // make sure to delete all bucket content before trying to delete the bucket
        if let Err(e) = self.empty_bucket(bucket_name) {
            return Err(e);
        }

        match block_on(s3_client.delete_bucket(DeleteBucketRequest {
            bucket: bucket_name.to_string(),
            expected_bucket_owner: None,
        })) {
            Ok(_) => Ok(()),
            Err(e) => Err(ObjectStorageError::CannotDeleteBucket {
                bucket_name: bucket_name.to_string(),
                raw_error_message: e.to_string(),
            }),
        }
    }

    fn get(
        &self,
        bucket_name: &str,
        object_key: &str,
        use_cache: bool,
    ) -> Result<(StringPath, File), ObjectStorageError> {
        let _ = S3::is_bucket_name_valid(bucket_name)?;

        let workspace_directory = crate::fs::workspace_directory(
            self.context().workspace_root_dir(),
            self.context().execution_id(),
            format!("object-storage/s3/{}", self.name()),
        )
        .map_err(|err| ObjectStorageError::CannotGetWorkspace {
            bucket_name: bucket_name.to_string(),
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
            expected_bucket_owner: None,
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
                        Err(e) => Err(ObjectStorageError::CannotCreateFile {
                            bucket_name: bucket_name.to_string(),
                            raw_error_message: e.to_string(),
                        }),
                    },
                    Err(e) => Err(ObjectStorageError::CannotOpenFile {
                        bucket_name: bucket_name.to_string(),
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
        let _ = S3::is_bucket_name_valid(bucket_name)?;

        let s3_client = self.get_s3_client();

        match block_on(s3_client.put_object(PutObjectRequest {
            bucket: bucket_name.to_string(),
            key: object_key.to_string(),
            body: Some(StreamingBody::from(match std::fs::read(file_path) {
                Ok(x) => x,
                Err(e) => {
                    return Err(ObjectStorageError::CannotReadFile {
                        bucket_name: bucket_name.to_string(),
                        raw_error_message: e.to_string(),
                    })
                }
            })),
            expected_bucket_owner: None,
            ..Default::default()
        })) {
            Ok(_) => Ok(()),
            Err(e) => Err(ObjectStorageError::CannotUploadFile {
                bucket_name: bucket_name.to_string(),
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

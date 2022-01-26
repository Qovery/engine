use chrono::Utc;
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

use crate::error::{EngineError, EngineErrorCause};
use crate::models::{Context, StringPath};
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
            access_key_id: access_key_id.to_string(),
            secret_access_key: secret_access_key.to_string(),
            region: region.clone(),
            bucket_versioning_activated,
            bucket_ttl_in_seconds,
        }
    }

    fn get_credentials(&self) -> StaticProvider {
        StaticProvider::new(self.access_key_id.clone(), self.secret_access_key.clone(), None, None)
    }

    fn get_s3_client(&self) -> S3Client {
        let region = RusotoRegion::from_str(&self.region.to_aws_format())
            .expect(format!("S3 region `{}` doesn't seems to be valid.", self.region.to_aws_format()).as_str());
        let client = Client::new_with(
            self.get_credentials(),
            HttpClient::new().expect("unable to create new Http client"),
        );

        S3Client::new_with_client(client, region)
    }

    fn is_bucket_name_valid(bucket_name: &str) -> Result<(), Option<String>> {
        if bucket_name.is_empty() {
            return Err(Some("bucket name cannot be empty".to_string()));
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

    fn empty_bucket(&self, bucket_name: &str) -> Result<(), EngineError> {
        if let Err(message) = S3::is_bucket_name_valid(bucket_name) {
            let message = format!(
                "While trying to empty S3 bucket, name `{}` is invalid: {}",
                bucket_name,
                message.unwrap_or_else(|| "unknown error".to_string())
            );
            return Err(self.engine_error(EngineErrorCause::Internal, message));
        }

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
                let message = format!(
                    "While trying to empty S3 bucket `{}` region `{}`, cannot delete content: {}",
                    bucket_name,
                    self.region.to_aws_format(),
                    e
                );
                error!("{}", message);
                return Err(self.engine_error(EngineErrorCause::Internal, message));
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

    fn is_valid(&self) -> Result<(), EngineError> {
        // TODO check valid credentials
        Ok(())
    }

    fn create_bucket(&self, bucket_name: &str) -> Result<(), EngineError> {
        if let Err(message) = S3::is_bucket_name_valid(bucket_name) {
            let message = format!(
                "While trying to create S3 bucket, name `{}` is invalid: {}",
                bucket_name,
                message.unwrap_or_else(|| "unknown error".to_string())
            );
            return Err(self.engine_error(EngineErrorCause::Internal, message));
        }

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
            let message = format!(
                "While trying to create S3 bucket, name `{}` region `{}`: {}",
                bucket_name,
                self.region.to_aws_format(),
                e
            );
            error!("{}", message);
            return Err(self.engine_error(EngineErrorCause::Internal, message));
        }

        let mut bucket_tags = vec![Tag {
            key: "CreationDate".to_string(),
            value: Utc::now().to_rfc3339().to_string(),
        }];
        if let Some(bucket_ttl) = self.bucket_ttl_in_seconds {
            bucket_tags.push(Tag {
                key: "Ttl".to_string(),
                value: bucket_ttl.to_string(),
            });
        }

        if let Err(e) = block_on(s3_client.put_bucket_tagging(PutBucketTaggingRequest {
            bucket: bucket_name.to_string(),
            expected_bucket_owner: None,
            tagging: Tagging { tag_set: bucket_tags },
            ..Default::default()
        })) {
            let message = format!(
                "While trying to add tags on S3 bucket, name `{}` region `{}`: {}",
                bucket_name,
                self.region.to_aws_format(),
                e
            );
            error!("{}", message);
            return Err(self.engine_error(EngineErrorCause::Internal, message));
        }

        if self.bucket_versioning_activated {
            if let Err(e) = block_on(s3_client.put_bucket_versioning(PutBucketVersioningRequest {
                bucket: bucket_name.to_string(),
                ..Default::default()
            })) {
                let message = format!(
                    "While trying to activate versioning on S3 bucket, name `{}` region `{}`: {}",
                    bucket_name,
                    self.region.to_aws_format(),
                    e
                );
                error!("{}", message);
            }
        }

        Ok(())
    }

    fn delete_bucket(&self, bucket_name: &str) -> Result<(), EngineError> {
        if let Err(message) = S3::is_bucket_name_valid(bucket_name) {
            let message = format!(
                "While trying to delete S3 bucket, name `{}` is invalid: {}",
                bucket_name,
                message.unwrap_or_else(|| "unknown error".to_string())
            );
            return Err(self.engine_error(EngineErrorCause::Internal, message));
        }

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
            Err(e) => {
                let message = format!(
                    "While trying to delete S3 bucket, name `{}` region `{}`: {}",
                    bucket_name,
                    self.region.to_aws_format(),
                    e
                );
                error!("{}", message);
                Err(self.engine_error(EngineErrorCause::Internal, message))
            }
        }
    }

    fn get(&self, bucket_name: &str, object_key: &str, use_cache: bool) -> Result<(StringPath, File), EngineError> {
        if let Err(message) = S3::is_bucket_name_valid(bucket_name) {
            let message = format!(
                "While trying to get object `{}` from bucket `{}`, bucket name is invalid: {}",
                object_key,
                bucket_name,
                message.unwrap_or_else(|| "unknown error".to_string())
            );
            return Err(self.engine_error(EngineErrorCause::Internal, message));
        }

        let workspace_directory = crate::fs::workspace_directory(
            self.context().workspace_root_dir(),
            self.context().execution_id(),
            format!("object-storage/s3/{}", self.name()),
        )
        .map_err(|err| self.engine_error(EngineErrorCause::Internal, err.to_string()))?;

        let file_path = format!("{}/{}/{}", workspace_directory, bucket_name, object_key);

        if use_cache {
            // does config file already exists?
            match File::open(file_path.as_str()) {
                Ok(file) => {
                    debug!("{} cache hit", file_path.as_str());
                    return Ok((file_path, file));
                }
                Err(_) => debug!("{} cache miss", file_path.as_str()),
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
                        Err(e) => {
                            let message = format!("{}", e);
                            error!("{}", message);
                            Err(self.engine_error(EngineErrorCause::Internal, message))
                        }
                    },
                    Err(e) => {
                        let message = format!("{}", e);
                        error!("{}", message);
                        Err(self.engine_error(EngineErrorCause::Internal, message))
                    }
                }
            }
            Err(e) => {
                let message = format!(
                    "While trying to get object `{}` from bucket `{}` region `{}`, error: {}",
                    object_key,
                    bucket_name,
                    self.region.to_aws_format(),
                    e
                );
                error!("{}", message);
                Err(self.engine_error(EngineErrorCause::Internal, message))
            }
        }
    }

    fn put(&self, bucket_name: &str, object_key: &str, file_path: &str) -> Result<(), EngineError> {
        if let Err(message) = S3::is_bucket_name_valid(bucket_name) {
            let message = format!(
                "While trying to get object `{}` from bucket `{}`, bucket name is invalid: {}",
                object_key,
                bucket_name,
                message.unwrap_or_else(|| "unknown error".to_string())
            );
            return Err(self.engine_error(EngineErrorCause::Internal, message));
        }

        let s3_client = self.get_s3_client();

        match block_on(s3_client.put_object(PutObjectRequest {
            bucket: bucket_name.to_string(),
            key: object_key.to_string(),
            body: Some(StreamingBody::from(match std::fs::read(file_path.clone()) {
                Ok(x) => x,
                Err(e) => {
                    return Err(self.engine_error(
                        EngineErrorCause::Internal,
                        format!(
                            "error while uploading object {} to bucket {}. {}",
                            object_key, bucket_name, e
                        ),
                    ))
                }
            })),
            expected_bucket_owner: None,
            ..Default::default()
        })) {
            Ok(_) => Ok(()),
            Err(e) => {
                let message = format!(
                    "While trying to put object `{}` from bucket `{}` region `{}`, error: {}",
                    object_key,
                    bucket_name,
                    self.region.to_aws_format(),
                    e
                );
                error!("{}", message);
                Err(self.engine_error(EngineErrorCause::Internal, message))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestCase<'a> {
        bucket_name_input: &'a str,
        expected_output: Result<(), Option<String>>,
        description: &'a str,
    }

    #[test]
    fn test_is_bucket_name_valid() {
        // setup:
        let test_cases: Vec<TestCase> = vec![
            TestCase {
                bucket_name_input: "",
                expected_output: Err(Some(String::from("bucket name cannot be empty"))),
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

use std::fs::File;
use std::path::Path;

use crate::cloud_provider::scaleway::application::Region;
use crate::error::{EngineError, EngineErrorCause};
use crate::models::{Context, StringPath};
use crate::object_storage::{Kind, ObjectStorage};

use crate::runtime::block_on;
use rusoto_core::{Client, HttpClient, Region as RusotoRegion};
use rusoto_credential::StaticProvider;
use rusoto_s3::{
    CreateBucketRequest, Delete, DeleteBucketRequest, DeleteObjectsRequest, GetObjectRequest, HeadBucketRequest,
    ListObjectsRequest, ObjectIdentifier, PutBucketVersioningRequest, PutObjectRequest, S3Client, StreamingBody, S3,
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
    region: Region,
    bucket_delete_strategy: BucketDeleteStrategy,
}

impl ScalewayOS {
    pub fn new(
        context: Context,
        id: String,
        name: String,
        access_key: String,
        secret_token: String,
        region: Region,
        bucket_delete_strategy: BucketDeleteStrategy,
    ) -> ScalewayOS {
        ScalewayOS {
            context,
            id,
            name,
            access_key,
            secret_token,
            region,
            bucket_delete_strategy,
        }
    }

    fn get_s3_client(&self) -> S3Client {
        let region = RusotoRegion::Custom {
            name: self.region.to_string(),
            endpoint: self.get_endpoint_url_for_region(),
        };

        let client = Client::new_with(self.get_credentials(), HttpClient::new().unwrap());

        S3Client::new_with_client(client, region)
    }

    fn get_credentials(&self) -> StaticProvider {
        StaticProvider::new(self.access_key.clone(), self.secret_token.clone(), None, None)
    }

    fn get_endpoint_url_for_region(&self) -> String {
        format!("https://s3.{}.scw.cloud", self.region.to_string())
    }

    fn is_bucket_name_valid(bucket_name: &str) -> Result<(), Option<String>> {
        if bucket_name.is_empty() {
            return Err(Some("bucket name cannot be empty".to_string()));
        }
        // From Scaleway doc
        // Note: The SSL certificate does not support bucket names containing additional dots (.).
        // You may receive a SSL warning in your browser when accessing a bucket like my.bucket.name.s3.fr-par.scw.cloud
        // and it is recommended to use dashes (-) instead: my-bucket-name.s3.fr-par.scw.cloud.
        if bucket_name.contains('.') {
            return Err(Some(
                "bucket name cannot contain '.' in its name, recommended to use '-' instead".to_string(),
            ));
        }

        Ok(())
    }

    fn empty_bucket(&self, bucket_name: &str) -> Result<(), EngineError> {
        // TODO(benjamin): switch to `scaleway-api-rs` once object storage will be supported (https://github.com/Qovery/scaleway-api-rs/issues/12).
        if let Err(message) = ScalewayOS::is_bucket_name_valid(bucket_name) {
            let message = format!(
                "While trying to delete object-storage bucket, name `{}` is invalid: {}",
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
                    "While trying to delete object-storage bucket `{}`, cannot delete content: {}",
                    bucket_name, e
                );
                error!("{}", message);
                return Err(self.engine_error(EngineErrorCause::Internal, message));
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

    fn is_valid(&self) -> Result<(), EngineError> {
        todo!()
    }

    fn create_bucket(&self, bucket_name: &str) -> Result<(), EngineError> {
        // TODO(benjamin): switch to `scaleway-api-rs` once object storage will be supported (https://github.com/Qovery/scaleway-api-rs/issues/12).
        if let Err(message) = ScalewayOS::is_bucket_name_valid(bucket_name) {
            let message = format!(
                "While trying to create object-storage bucket, name `{}` is invalid: {}",
                bucket_name,
                message.unwrap_or_else(|| "unknown error".to_string())
            );
            return Err(self.engine_error(EngineErrorCause::Internal, message));
        }

        let s3_client = self.get_s3_client();

        // check if bucket already exists, if so, no need to recreate it
        // note: we are not deleting buckets since it takes up to 24 hours to be taken into account
        // so we better reuse existing ones
        if self.bucket_exists(bucket_name) {
            return Ok(());
        }

        if let Err(e) = block_on(s3_client.create_bucket(CreateBucketRequest {
            bucket: bucket_name.to_string(),
            ..Default::default()
        })) {
            let message = format!(
                "While trying to create object-storage bucket, name `{}`: {}",
                bucket_name, e
            );
            error!("{}", message);
            return Err(self.engine_error(EngineErrorCause::Internal, message));
        }

        match block_on(s3_client.put_bucket_versioning(PutBucketVersioningRequest {
            bucket: bucket_name.to_string(),
            ..Default::default()
        })) {
            Ok(_) => Ok(()),
            Err(e) => {
                let message = format!(
                    "While trying to activate versioning on object-storage bucket, name `{}`: {}",
                    bucket_name, e
                );
                error!("{}", message);
                // TODO(benjaminch): to be investigated, versioning seems to fail
                // Err(self.engine_error(EngineErrorCause::Internal, message))
                Ok(())
            }
        }
    }

    fn delete_bucket(&self, bucket_name: &str) -> Result<(), EngineError> {
        // TODO(benjamin): switch to `scaleway-api-rs` once object storage will be supported (https://github.com/Qovery/scaleway-api-rs/issues/12).
        if let Err(message) = ScalewayOS::is_bucket_name_valid(bucket_name) {
            let message = format!(
                "While trying to delete object-storage bucket, name `{}` is invalid: {}",
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

        // Note: Do not delete the bucket entirely but empty its content.
        // Bucket deletion might take up to 24 hours and during this time we are not able to create a bucket with the same name.
        // So emptying bucket allows future reuse.
        return match &self.bucket_delete_strategy {
            BucketDeleteStrategy::HardDelete => match block_on(s3_client.delete_bucket(DeleteBucketRequest {
                bucket: bucket_name.to_string(),
                ..Default::default()
            })) {
                Ok(_) => Ok(()),
                Err(e) => {
                    let message = format!(
                        "While trying to delete object-storage bucket, name `{}`: {}",
                        bucket_name, e
                    );
                    error!("{}", message);
                    return Err(self.engine_error(EngineErrorCause::Internal, message));
                }
            },
            BucketDeleteStrategy::Empty => Ok(()), // Do not delete the bucket
        };
    }

    fn get(&self, bucket_name: &str, object_key: &str, use_cache: bool) -> Result<(StringPath, File), EngineError> {
        // TODO(benjamin): switch to `scaleway-api-rs` once object storage will be supported (https://github.com/Qovery/scaleway-api-rs/issues/12).
        if let Err(message) = ScalewayOS::is_bucket_name_valid(bucket_name) {
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
            format!("object-storage/scaleway_os/{}", self.name()),
        );

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
                match block_on(tokio::fs::File::create(path)) {
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
                    "While trying to get object `{}` from bucket `{}`, error: {}",
                    object_key, bucket_name, e
                );
                error!("{}", message);
                Err(self.engine_error(EngineErrorCause::Internal, message))
            }
        }
    }

    fn put(&self, bucket_name: &str, object_key: &str, file_path: &str) -> Result<(), EngineError> {
        // TODO(benjamin): switch to `scaleway-api-rs` once object storage will be supported (https://github.com/Qovery/scaleway-api-rs/issues/12).
        if let Err(message) = ScalewayOS::is_bucket_name_valid(bucket_name) {
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
            body: Some(StreamingBody::from(std::fs::read(file_path.clone()).unwrap())),
            ..Default::default()
        })) {
            Ok(_) => Ok(()),
            Err(e) => {
                let message = format!(
                    "While trying to put object `{}` from bucket `{}`, error: {}",
                    object_key, bucket_name, e
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
                bucket_name_input: "containing.dot",
                expected_output: Err(Some(String::from(
                    "bucket name cannot contain '.' in its name, recommended to use '-' instead",
                ))),
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
            assert_eq!(tc.expected_output, result);
        }
    }
}

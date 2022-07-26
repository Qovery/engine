use std::fs::File;
use std::path::Path;

use retry::delay::Fibonacci;
use retry::{Error, OperationResult};
use rusoto_core::{Client, HttpClient, Region};
use rusoto_credential::StaticProvider;
use rusoto_s3::{
    CreateBucketRequest, Delete, DeleteBucketRequest, DeleteObjectRequest, DeleteObjectsRequest, GetObjectRequest,
    HeadBucketRequest, ListObjectsRequest, ObjectIdentifier, PutObjectRequest, S3Client, StreamingBody, S3,
};
use tokio::io;

use crate::io_models::context::Context;
use crate::io_models::domain::StringPath;
use crate::models::digital_ocean::DoRegion;
use crate::object_storage::errors::ObjectStorageError;
use crate::object_storage::{Kind, ObjectStorage};
use crate::runtime::block_on;

pub enum BucketDeleteStrategy {
    HardDelete,
    Empty,
}

pub struct Spaces {
    context: Context,
    id: String,
    name: String,
    access_key_id: String,
    secret_access_key: String,
    region: DoRegion,
    bucket_delete_strategy: BucketDeleteStrategy,
}

impl Spaces {
    pub fn new(
        context: Context,
        id: String,
        name: String,
        access_key_id: String,
        secret_access_key: String,
        region: DoRegion,
        bucket_delete_strategy: BucketDeleteStrategy,
    ) -> Self {
        Spaces {
            context,
            id,
            name,
            access_key_id,
            secret_access_key,
            region,
            bucket_delete_strategy,
        }
    }

    fn get_endpoint_url_for_region(&self) -> String {
        format!("https://{}.digitaloceanspaces.com", self.region)
    }

    fn get_credentials(&self) -> StaticProvider {
        StaticProvider::new(self.access_key_id.clone(), self.secret_access_key.clone(), None, None)
    }

    fn get_s3_client(&self) -> S3Client {
        let region = Region::Custom {
            name: self.region.to_string(),
            endpoint: self.get_endpoint_url_for_region(),
        };

        let credentials = self.get_credentials();
        let client = Client::new_with(credentials, HttpClient::new().unwrap());

        S3Client::new_with_client(client, region)
    }

    fn is_bucket_name_valid(bucket_name: &str) -> Result<(), ObjectStorageError> {
        if bucket_name.is_empty() {
            return Err(ObjectStorageError::InvalidBucketName {
                bucket_name: bucket_name.to_string(),
                raw_error_message: "bucket name cannot be empty".to_string(),
            });
        }

        if bucket_name.contains('.') {
            return Err(ObjectStorageError::InvalidBucketName {
                bucket_name: bucket_name.to_string(),
                raw_error_message: "bucket name cannot contain '.' in its name, recommended to use '-' instead"
                    .to_string(),
            });
        }

        Ok(())
    }

    pub fn empty_bucket(&self, bucket_name: &str) -> Result<(), ObjectStorageError> {
        Spaces::is_bucket_name_valid(bucket_name)?;

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

    async fn get_object<T, S, X>(
        &self,
        bucket_name: T,
        object_key: S,
        download_into_file_path: X,
    ) -> Result<File, ObjectStorageError>
    where
        T: Into<String>,
        S: Into<String>,
        X: AsRef<Path>,
    {
        let region = Region::Custom {
            name: self.region.to_string(),
            endpoint: format!("https://{}.digitaloceanspaces.com", self.region),
        };

        let credentials = StaticProvider::new(self.access_key_id.clone(), self.secret_access_key.clone(), None, None);

        let client = Client::new_with(credentials, HttpClient::new().unwrap());
        let s3_client = S3Client::new_with_client(client, region.clone());
        let bucket_name: String = bucket_name.into();
        let object_key: String = object_key.into();
        let object = s3_client
            .get_object(GetObjectRequest {
                bucket: bucket_name.to_string(),
                key: object_key.to_string(),
                ..Default::default()
            })
            .await;

        match object {
            Ok(mut obj_bod) => {
                let body = obj_bod.body.take();
                let mut body = body.unwrap().into_async_read();

                // create parent dir
                let path = download_into_file_path.as_ref();
                let parent_dir = path.parent().unwrap();
                let _ = tokio::fs::create_dir_all(parent_dir).await;

                // create file
                let file = tokio::fs::File::create(download_into_file_path.as_ref()).await;

                match file {
                    Ok(mut created_file) => match io::copy(&mut body, &mut created_file).await {
                        Ok(_) => Ok(File::open(download_into_file_path.as_ref()).unwrap()),
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
}

impl ObjectStorage for Spaces {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::Spaces
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
        Spaces::is_bucket_name_valid(bucket_name)?;

        let s3_client = self.get_s3_client();

        // check if bucket already exists, if so, no need to recreate it
        // note: we are not deleting buckets since it takes up to 3/4 weeks to be taken into account
        // so we better reuse existing ones
        if self.bucket_exists(bucket_name) {
            return Ok(());
        }

        if let Err(e) = block_on(s3_client.create_bucket(CreateBucketRequest {
            bucket: bucket_name.to_string(),
            ..Default::default()
        })) {
            return Err(ObjectStorageError::CannotCreateBucket {
                bucket_name: bucket_name.to_string(),
                raw_error_message: e.to_string(),
            });
        }

        Ok(())
    }

    fn delete_bucket(&self, bucket_name: &str) -> Result<(), ObjectStorageError> {
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
                    return Err(ObjectStorageError::CannotDeleteBucket {
                        bucket_name: bucket_name.to_string(),
                        raw_error_message: e.to_string(),
                    });
                }
            },
            BucketDeleteStrategy::Empty => Ok(()), // Do not delete the bucket
        };
    }

    fn get(
        &self,
        bucket_name: &str,
        object_key: &str,
        use_cache: bool,
    ) -> Result<(StringPath, File), ObjectStorageError> {
        let workspace_directory = crate::fs::workspace_directory(
            self.context().workspace_root_dir(),
            self.context().execution_id(),
            format!("object-storage/spaces/{}", self.name()),
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

        // retrieve config file from object storage
        let result = retry::retry(Fibonacci::from_millis(3000).take(5), || {
            match block_on(self.get_object(bucket_name, object_key, file_path.as_str())) {
                Ok(file) => OperationResult::Ok(file),
                Err(err) => OperationResult::Retry(err),
            }
        });

        let file = match result {
            Ok(_) => File::open(file_path.as_str()),
            Err(err) => {
                return match err {
                    Error::Operation { error, .. } => Err(error),
                    Error::Internal(err) => Err(ObjectStorageError::CannotGetObjectFile {
                        bucket_name: bucket_name.to_string(),
                        file_name: object_key.to_string(),
                        raw_error_message: err,
                    }),
                };
            }
        };

        match file {
            Ok(file) => Ok((file_path, file)),
            Err(err) => Err(ObjectStorageError::CannotGetObjectFile {
                bucket_name: bucket_name.to_string(),
                file_name: object_key.to_string(),
                raw_error_message: err.to_string(),
            }),
        }
    }

    fn put(&self, bucket_name: &str, object_key: &str, file_path: &str) -> Result<(), ObjectStorageError> {
        // TODO(benjamin): switch to `digitalocean-api-rs` once we'll made the auo-generated lib
        Spaces::is_bucket_name_valid(bucket_name)?;

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
        if Spaces::is_bucket_name_valid(bucket_name).is_err() {
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

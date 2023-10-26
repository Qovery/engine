use crate::io_models::context::Context;
use crate::models::domain::StringPath;
use crate::object_storage::errors::ObjectStorageError;
use crate::object_storage::{Kind, ObjectStorage};
use crate::runtime::block_on;
use crate::services::gcp::object_storage_regions::GcpStorageRegion;
use crate::services::gcp::object_storage_service::ObjectStorageService;
use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;

pub struct GoogleOS {
    context: Context,
    id: String,
    name: String,
    project_id: String,
    region: GcpStorageRegion,
    bucket_ttl: Option<Duration>,
    service: Arc<ObjectStorageService>,
}

impl GoogleOS {
    pub fn new(
        context: Context,
        id: &str,
        name: &str,
        project_id: &str,
        region: GcpStorageRegion,
        bucket_ttl: Option<Duration>,
        service: Arc<ObjectStorageService>,
    ) -> GoogleOS {
        Self {
            context,
            id: id.to_string(),
            name: name.to_string(),
            project_id: project_id.to_string(),
            region,
            bucket_ttl,
            service,
        }
    }
}

impl ObjectStorage for GoogleOS {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::GcpOs
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn is_valid(&self) -> Result<(), ObjectStorageError> {
        // TODO check valid credentials
        Ok(())
    }

    fn create_bucket(&self, bucket_name: &str) -> Result<(), ObjectStorageError> {
        let creation_date: DateTime<Utc> = Utc::now();
        match self.service.create_bucket(
            self.project_id.as_str(),
            bucket_name,
            self.region.clone(),
            self.bucket_ttl,
            Some(HashMap::from([
                ("CreationDate".to_string(), creation_date.to_rfc3339()),
                (
                    "Ttl".to_string(),
                    format!("{}", self.bucket_ttl.map(|ttl| ttl.num_seconds()).unwrap_or(0)),
                ),
            ])),
        ) {
            Ok(_) => Ok(()),
            Err(e) => Err(ObjectStorageError::CannotCreateBucket {
                bucket_name: bucket_name.to_string(),
                raw_error_message: e.to_string(),
            }),
        }
    }

    fn delete_bucket(&self, bucket_name: &str) -> Result<(), ObjectStorageError> {
        self.service
            .delete_bucket(bucket_name, true)
            .map_err(|e| ObjectStorageError::CannotDeleteBucket {
                bucket_name: bucket_name.to_string(),
                raw_error_message: e.to_string(),
            })
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
            format!("object-storage/google_os/{}", self.name()),
        )
        .map_err(|err| ObjectStorageError::CannotGetObjectFile {
            bucket_name: bucket_name.to_string(),
            file_name: object_key.to_string(),
            raw_error_message: err.to_string(),
        })?;

        let file_path = format!("{workspace_directory}/{bucket_name}/{object_key}");

        if use_cache {
            // does config file already exists?
            if let Ok(file) = File::open(file_path.as_str()) {
                return Ok((file_path, file));
            }
        }

        match self.service.get_object(bucket_name, object_key) {
            Err(e) => Err(ObjectStorageError::CannotGetObjectFile {
                bucket_name: bucket_name.to_string(),
                file_name: object_key.to_string(),
                raw_error_message: e.to_string(),
            }),
            Ok(object) => {
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
                    Ok(mut created_file) => {
                        if let Err(e) = block_on(created_file.write_all(object.value.as_slice())) {
                            return Err(ObjectStorageError::CannotGetObjectFile {
                                bucket_name: bucket_name.to_string(),
                                file_name: object_key.to_string(),
                                raw_error_message: e.to_string(),
                            });
                        }
                        Ok((file_path, block_on(created_file.into_std())))
                    }
                    Err(e) => Err(ObjectStorageError::CannotGetObjectFile {
                        bucket_name: bucket_name.to_string(),
                        file_name: object_key.to_string(),
                        raw_error_message: e.to_string(),
                    }),
                }
            }
        }
    }

    fn put(&self, bucket_name: &str, object_key: &str, file_path: &str) -> Result<(), ObjectStorageError> {
        let file_content = std::fs::read(file_path).map_err(|e| ObjectStorageError::CannotUploadFile {
            bucket_name: bucket_name.to_string(),
            file_name: object_key.to_string(),
            raw_error_message: e.to_string(),
        })?;

        match self.service.put_object(bucket_name, object_key, file_content) {
            Ok(_) => Ok(()),
            Err(e) => Err(ObjectStorageError::CannotUploadFile {
                bucket_name: bucket_name.to_string(),
                file_name: object_key.to_string(),
                raw_error_message: e.to_string(),
            }),
        }
    }

    fn delete(&self, bucket_name: &str, object_key: &str) -> Result<(), ObjectStorageError> {
        // TODO(benjaminch): not optimal since fine grained statuses are not returned, should know if get is error because file doesn't exist or if anything else
        if self.get(bucket_name, object_key, false).is_err() {
            return Ok(());
        }

        self.service
            .delete_object(bucket_name, object_key)
            .map_err(|e| ObjectStorageError::CannotDeleteFile {
                bucket_name: bucket_name.to_string(),
                file_name: object_key.to_string(),
                raw_error_message: e.to_string(),
            })
    }
}

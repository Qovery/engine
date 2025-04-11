use crate::infrastructure::models::object_storage::errors::ObjectStorageError;
use crate::infrastructure::models::object_storage::{Bucket, BucketDeleteStrategy, BucketObject};
use crate::services;
use crate::services::azure::blob_storage_regions::AzureStorageRegion;
use crate::services::azure::blob_storage_service::BlobStorageService;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageAccount {
    pub access_key: String,
    pub account_name: String,
}

impl StorageAccount {
    pub fn to_storage_account_service_model(&self) -> services::azure::blob_storage_service::StorageAccount {
        services::azure::blob_storage_service::StorageAccount {
            access_key: self.access_key.to_string(),
            account_name: self.account_name.to_string(),
        }
    }
}

pub struct AzureOS {
    id: String,
    _long_id: Uuid,
    name: String,

    service: Arc<BlobStorageService>,
}

impl AzureOS {
    pub fn new(id: &str, long_id: Uuid, name: &str, service: Arc<BlobStorageService>) -> Self {
        Self {
            id: id.to_string(),
            _long_id: long_id,
            name: name.to_string(),

            service,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn is_valid(&self) -> Result<(), ObjectStorageError> {
        // TODO check valid credentials
        Ok(())
    }

    pub fn bucket_exists(&self, storage_account: &StorageAccount, bucket_name: &str) -> bool {
        self.service.bucket_exists(
            &storage_account.to_storage_account_service_model(),
            bucket_name,
            &AzureStorageRegion::Public {
                account: storage_account.account_name.to_string(),
            },
        )
    }

    pub fn create_bucket(
        &self,
        storage_account: &StorageAccount,
        bucket_name: &str,
        bucket_ttl: Option<Duration>,
        _bucket_versioning_activated: bool, // TODO(benjaminch): Add bucket versioning option
        _bucket_logging_activated: bool,    // TODO(benjaminch): Add bucket logging option
    ) -> Result<Bucket, ObjectStorageError> {
        if let Ok(existing_bucket) = self.get_bucket(storage_account, bucket_name) {
            return Ok(existing_bucket);
        }

        let creation_date: DateTime<Utc> = Utc::now();
        // TODO(benjaminch): Add bucket versioning option
        // TODO(benjaminch): Add bucket logging option
        match self.service.create_bucket(
            &storage_account.to_storage_account_service_model(),
            bucket_name,
            &AzureStorageRegion::Public {
                account: storage_account.account_name.to_string(),
            },
            bucket_ttl,
            Some(HashMap::from([
                ("creation_date".to_string(), creation_date.timestamp().to_string()),
                (
                    "ttl".to_string(),
                    format!("{}", bucket_ttl.map(|ttl| ttl.as_secs()).unwrap_or(0)),
                ),
            ])),
        ) {
            Ok(o) => Ok(o),
            Err(e) => Err(ObjectStorageError::CannotCreateBucket {
                bucket_name: bucket_name.to_string(),
                raw_error_message: e.to_string(),
            }),
        }
    }

    pub fn update_bucket(
        &self,
        storage_account: &StorageAccount,
        bucket_name: &str,
        bucket_ttl: Option<Duration>,
        bucket_versioning_activated: bool,
        bucket_logging_activated: bool,
        bucket_labels: Option<HashMap<String, String>>,
    ) -> Result<Bucket, ObjectStorageError> {
        if let Err(err) = self.get_bucket(storage_account, bucket_name) {
            return Err(ObjectStorageError::CannotUpdateBucket {
                bucket_name: bucket_name.to_string(),
                raw_error_message: err.to_string(),
            });
        }

        // Update the bucket
        match self.service.update_bucket(
            &storage_account.to_storage_account_service_model(),
            bucket_name,
            &AzureStorageRegion::Public {
                account: storage_account.account_name.to_string(),
            },
            bucket_ttl,
            bucket_versioning_activated,
            bucket_logging_activated,
            bucket_labels,
        ) {
            Ok(o) => Ok(o),
            Err(e) => Err(ObjectStorageError::CannotUpdateBucket {
                bucket_name: bucket_name.to_string(),
                raw_error_message: e.to_string(),
            }),
        }
    }

    pub fn get_bucket(
        &self,
        storage_account: &StorageAccount,
        bucket_name: &str,
    ) -> Result<Bucket, ObjectStorageError> {
        match self.service.get_bucket(
            &storage_account.to_storage_account_service_model(),
            bucket_name,
            &AzureStorageRegion::Public {
                account: storage_account.account_name.to_string(),
            },
        ) {
            Ok(o) => Ok(o),
            Err(e) => Err(ObjectStorageError::CannotGetBucket {
                bucket_name: bucket_name.to_string(),
                raw_error_message: e.to_string(),
            }),
        }
    }

    pub fn delete_bucket(
        &self,
        storage_account: &StorageAccount,
        bucket_name: &str,
        _strategy: BucketDeleteStrategy,
    ) -> Result<(), ObjectStorageError> {
        self.service
            .delete_bucket(&storage_account.to_storage_account_service_model(), bucket_name)
            .map_err(|e| ObjectStorageError::CannotDeleteBucket {
                bucket_name: bucket_name.to_string(),
                raw_error_message: e.to_string(),
            })
    }

    pub fn delete_bucket_non_blocking(
        &self,
        _storage_account: &StorageAccount,
        _bucket_name: &str,
    ) -> Result<(), ObjectStorageError> {
        todo!("delete_bucket_non_blocking for Azure is not implemented")
    }

    pub fn get_object(
        &self,
        storage_account: &StorageAccount,
        bucket_name: &str,
        object_key: &str,
    ) -> Result<BucketObject, ObjectStorageError> {
        match self
            .service
            .get_object(&storage_account.to_storage_account_service_model(), bucket_name, object_key)
        {
            Err(e) => Err(ObjectStorageError::CannotGetObjectFile {
                bucket_name: bucket_name.to_string(),
                object_name: object_key.to_string(),
                raw_error_message: e.to_string(),
            }),
            Ok(object) => Ok(object),
        }
    }

    pub fn put_object(
        &self,
        storage_account: &StorageAccount,
        bucket_name: &str,
        object_key: &str,
        file_path: &Path,
        tags: Option<Vec<String>>,
    ) -> Result<BucketObject, ObjectStorageError> {
        let file_content = std::fs::read(file_path).map_err(|e| ObjectStorageError::CannotUploadFile {
            bucket_name: bucket_name.to_string(),
            object_name: object_key.to_string(),
            raw_error_message: e.to_string(),
        })?;

        match self.service.put_object(
            &storage_account.to_storage_account_service_model(),
            bucket_name,
            object_key,
            file_content,
            tags.map(|object_tags| {
                object_tags
                    .iter()
                    .map(|tag| {
                        let mut parts = tag.split('=');
                        let key = parts.next().unwrap_or_default();
                        let value = parts.next().unwrap_or_default();
                        (key.to_string(), value.to_string())
                    })
                    .collect()
            }),
        ) {
            Ok(object) => Ok(object),
            Err(e) => Err(ObjectStorageError::CannotUploadFile {
                bucket_name: bucket_name.to_string(),
                object_name: object_key.to_string(),
                raw_error_message: e.to_string(),
            }),
        }
    }

    pub fn delete_object(
        &self,
        storage_account: &StorageAccount,
        bucket_name: &str,
        object_key: &str,
    ) -> Result<(), ObjectStorageError> {
        // TODO(benjaminch): not optimal since fine grained statuses are not returned, should know if get is error because file doesn't exist or if anything else
        if self.get_object(storage_account, bucket_name, object_key).is_err() {
            return Ok(());
        }

        self.service
            .delete_object(&storage_account.to_storage_account_service_model(), bucket_name, object_key)
            .map_err(|e| ObjectStorageError::CannotDeleteFile {
                bucket_name: bucket_name.to_string(),
                object_name: object_key.to_string(),
                raw_error_message: e.to_string(),
            })
    }
}

#[cfg(test)]
mod tests {
    use crate::infrastructure::models::object_storage::azure_object_storage::{AzureOS, StorageAccount};
    use crate::infrastructure::models::object_storage::errors::ObjectStorageError;
    use crate::infrastructure::models::object_storage::{Bucket, BucketDeleteStrategy, BucketObject, BucketRegion};
    use crate::services::azure::blob_storage_regions::AzureStorageRegion;
    use crate::services::azure::blob_storage_service::{BlobStorageService, BlobStorageServiceError};
    use chrono::Utc;
    use itertools::izip;
    use std::collections::HashMap;
    use std::fs::File;
    use std::io::Write;
    use std::time::Duration;
    use strum::IntoEnumIterator;
    use tempfile::tempdir;

    #[test]
    fn bucket_exists_test() {
        // setup:
        let existing_bucket_name = "this-bucket-exists";
        let not_existing_bucket_name = "this-bucket-doesnt-exist";
        let storage_account = StorageAccount {
            account_name: "account_123".to_string(),
            access_key: "access_key_123".to_string(),
        };

        let mut service_mock = BlobStorageService::faux();
        faux::when!(service_mock.bucket_exists(_, existing_bucket_name, _)).then_return(true);
        faux::when!(service_mock.bucket_exists(_, not_existing_bucket_name, _)).then_return(false);

        let blob_storage = AzureOS::new("123", uuid::Uuid::new_v4(), "test_123", std::sync::Arc::new(service_mock));

        // execute & verify:
        assert!(blob_storage.bucket_exists(&storage_account, existing_bucket_name));
        assert!(!blob_storage.bucket_exists(&storage_account, not_existing_bucket_name));
    }

    #[test]
    fn create_bucket_success_test() {
        // setup:
        let storage_account = StorageAccount {
            account_name: "account_123".to_string(),
            access_key: "access_key_123".to_string(),
        };
        let bucket_location = AzureStorageRegion::Public {
            account: storage_account.account_name.to_string(),
        };
        let bucket_name_test_cases = vec!["abc", "abcabc", "ABC_abc"];
        let bucket_ttl_test_cases = vec![
            None,
            Some(Duration::from_secs(60 * 60 * 24)),     // 1 day
            Some(Duration::from_secs(7 * 60 * 60 * 24)), // 7 days
        ];
        let bucket_versioning_test_cases = vec![true, false];
        let bucket_logging_test_cases = vec![true, false];

        for (bucket_name, bucket_ttl, bucket_versionning, bucket_logging) in izip!(
            bucket_name_test_cases,
            bucket_ttl_test_cases,
            bucket_versioning_test_cases,
            bucket_logging_test_cases,
        ) {
            let expected_bucket = Bucket {
                name: bucket_name.to_string(),
                ttl: bucket_ttl,
                versioning_activated: false,
                logging_activated: false,
                location: BucketRegion::AzureRegion(bucket_location.clone()),
                labels: Some(HashMap::from([
                    ("CreationDate".to_string(), Utc::now().to_rfc3339()),
                    (
                        "Ttl".to_string(),
                        format!("{}", bucket_ttl.map(|ttl| ttl.as_secs()).unwrap_or(0)),
                    ),
                ])),
            };

            let mut service_mock = BlobStorageService::faux();
            faux::when!(service_mock.create_bucket(_, bucket_name, _, bucket_ttl, _))
                .then_return(Ok(expected_bucket.clone()));
            faux::when!(service_mock.get_bucket(_, bucket_name, _)).then_return(Err(
                BlobStorageServiceError::CannotGetBucket {
                    storage_account_name: storage_account.account_name.to_string(),
                    bucket_name: bucket_name.to_string(),
                    raw_error_message: "Bucket doesn't exist".to_string(),
                },
            ));

            let blob_storage = AzureOS::new("123", uuid::Uuid::new_v4(), "test_123", std::sync::Arc::new(service_mock));

            // execute:
            let created_bucket = blob_storage
                .create_bucket(&storage_account, bucket_name, bucket_ttl, bucket_versionning, bucket_logging)
                .expect("Error creating bucket");

            // verify:
            assert_eq!(expected_bucket, created_bucket);
        }
    }

    #[test]
    fn create_bucket_existing_test() {
        // setup:
        let storage_account = StorageAccount {
            account_name: "account_123".to_string(),
            access_key: "access_key_123".to_string(),
        };
        let bucket_location = AzureStorageRegion::Public {
            account: storage_account.account_name.to_string(),
        };
        let bucket_name_test_cases = vec!["abc", "abcabc", "ABC_abc"];
        let bucket_ttl_test_cases = vec![
            None,
            Some(Duration::from_secs(60 * 60 * 24)),     // 1 day
            Some(Duration::from_secs(7 * 60 * 60 * 24)), // 7 days
        ];
        let bucket_versioning_test_cases = vec![true, false];
        let bucket_logging_test_cases = vec![true, false];

        for (bucket_name, bucket_ttl, bucket_versionning, bucket_logging) in izip!(
            bucket_name_test_cases,
            bucket_ttl_test_cases,
            bucket_versioning_test_cases,
            bucket_logging_test_cases,
        ) {
            let expected_bucket = Bucket {
                name: bucket_name.to_string(),
                ttl: bucket_ttl,
                versioning_activated: false,
                logging_activated: false,
                location: BucketRegion::AzureRegion(bucket_location.clone()),
                labels: Some(HashMap::from([
                    ("CreationDate".to_string(), Utc::now().to_rfc3339()),
                    (
                        "Ttl".to_string(),
                        format!("{}", bucket_ttl.map(|ttl| ttl.as_secs()).unwrap_or(0)),
                    ),
                ])),
            };

            let mut service_mock = BlobStorageService::faux();
            // Make create service returns an error if called, making sure success comes from the get
            faux::when!(service_mock.create_bucket(_, bucket_name, _, bucket_ttl, _)).then_return(Err(
                BlobStorageServiceError::CannotCreateBucket {
                    storage_account_name: storage_account.account_name.to_string(),
                    bucket_name: bucket_name.to_string(),
                    raw_error_message: "Bucket doesn't exist".to_string(),
                },
            ));
            faux::when!(service_mock.get_bucket(_, bucket_name, _)).then_return(Ok(expected_bucket.clone()));

            let blob_storage = AzureOS::new("123", uuid::Uuid::new_v4(), "test_123", std::sync::Arc::new(service_mock));

            // execute:
            let created_bucket = blob_storage
                .create_bucket(&storage_account, bucket_name, bucket_ttl, bucket_versionning, bucket_logging)
                .expect("Error creating bucket");

            // verify:
            assert_eq!(expected_bucket, created_bucket);
        }
    }

    #[test]
    fn get_bucket_success_test() {
        // setup:
        let storage_account = StorageAccount {
            account_name: "account_123".to_string(),
            access_key: "access_key_123".to_string(),
        };
        let bucket_location = AzureStorageRegion::Public {
            account: storage_account.account_name.to_string(),
        };
        let bucket_name = "test-bucket";
        let bucket_ttl = Some(Duration::from_secs(7 * 24 * 60 * 60)); // 7 days
        let expected_bucket = Bucket {
            name: bucket_name.to_string(),
            ttl: bucket_ttl,
            versioning_activated: false,
            logging_activated: false,
            location: BucketRegion::AzureRegion(bucket_location.clone()),
            labels: Some(HashMap::from([
                ("CreationDate".to_string(), Utc::now().to_rfc3339()),
                (
                    "Ttl".to_string(),
                    format!("{}", bucket_ttl.map(|ttl| ttl.as_secs()).unwrap_or(0)),
                ),
            ])),
        };

        let mut service_mock = BlobStorageService::faux();
        faux::when!(service_mock.get_bucket(_, bucket_name, _)).then_return(Ok(expected_bucket.clone()));

        let blob_storage = AzureOS::new("123", uuid::Uuid::new_v4(), "test_123", std::sync::Arc::new(service_mock));

        // execute:
        let retrieved_bucket = blob_storage.get_bucket(&storage_account, bucket_name);

        // verify:
        assert_eq!(Ok(expected_bucket), retrieved_bucket);
    }

    #[test]
    fn get_bucket_failure_test() {
        // setup:
        let storage_account = StorageAccount {
            account_name: "account_123".to_string(),
            access_key: "access_key_123".to_string(),
        };
        let bucket_name = "test-bucket";
        let raw_error_message = "get error message";

        let mut service_mock = BlobStorageService::faux();
        faux::when!(service_mock.get_bucket(_, bucket_name, _)).then_return(Err(
            BlobStorageServiceError::CannotGetBucket {
                storage_account_name: storage_account.account_name.to_string(),
                bucket_name: bucket_name.to_string(),
                raw_error_message: raw_error_message.to_string(),
            },
        ));

        let blob_storage = AzureOS::new("123", uuid::Uuid::new_v4(), "test_123", std::sync::Arc::new(service_mock));

        // execute:
        let retrieved_bucket = blob_storage.get_bucket(&storage_account, bucket_name);

        // verify:
        assert_eq!(
            ObjectStorageError::CannotGetBucket {
                bucket_name: bucket_name.to_string(),
                raw_error_message: format!(
                    "Cannot get bucket `{}` from storage account `{}`: \"{}\"",
                    bucket_name, storage_account.account_name, raw_error_message
                ),
            },
            retrieved_bucket.unwrap_err()
        );
    }

    #[test]
    #[ignore = "TODO(benjaminch): Implement update_bucket for AzureOS"]
    fn update_bucket_success_test() {}

    #[test]
    #[ignore = "TODO(benjaminch): Implement update_bucket for AzureOS"]
    fn update_bucket_failure_test() {}

    #[test]
    fn delete_bucket_success_test() {
        // setup:
        let storage_account = StorageAccount {
            account_name: "account_123".to_string(),
            access_key: "access_key_123".to_string(),
        };
        let bucket_name = "test-bucket";

        let mut service_mock = BlobStorageService::faux();
        faux::when!(service_mock.delete_bucket(_, bucket_name)).then_return(Ok(()));

        let blob_storage = AzureOS::new("123", uuid::Uuid::new_v4(), "test_123", std::sync::Arc::new(service_mock));

        for delete_strategy in BucketDeleteStrategy::iter() {
            // execute:
            let delete_result = blob_storage.delete_bucket(&storage_account, bucket_name, delete_strategy);

            // verify:
            assert_eq!(Ok(()), delete_result);
        }
    }

    #[test]
    fn delete_bucket_failure_test() {
        // setup:
        let storage_account = StorageAccount {
            account_name: "account_123".to_string(),
            access_key: "access_key_123".to_string(),
        };
        let bucket_name = "test-bucket";
        let raw_error_message = "delete error message";

        let mut service_mock = BlobStorageService::faux();
        faux::when!(service_mock.delete_bucket(_, bucket_name)).then_return(Err(
            BlobStorageServiceError::CannotDeleteBucket {
                storage_account_name: storage_account.account_name.to_string(),
                bucket_name: bucket_name.to_string(),
                raw_error_message: raw_error_message.to_string(),
            },
        ));

        let blob_storage = AzureOS::new("123", uuid::Uuid::new_v4(), "test_123", std::sync::Arc::new(service_mock));

        for delete_strategy in BucketDeleteStrategy::iter() {
            // execute:
            let delete_result = blob_storage.delete_bucket(&storage_account, bucket_name, delete_strategy);

            // verify:
            assert_eq!(
                ObjectStorageError::CannotDeleteBucket {
                    bucket_name: bucket_name.to_string(),
                    raw_error_message: format!(
                        "Cannot delete bucket `{}` from storage account `{}`: \"{}\"",
                        bucket_name, storage_account.account_name, raw_error_message
                    ),
                },
                delete_result.unwrap_err(),
            );
        }
    }

    #[test]
    fn put_object_success_test() {
        // setup:
        let bucket_name = "test-bucket";
        let storage_account = StorageAccount {
            account_name: "account_123".to_string(),
            access_key: "access_key_123".to_string(),
        };
        let object_key = "test-object-key";
        let object_content = "test-object-content";
        let object_tags = vec!["tag1=value1".to_string(), "tag2=value2".to_string()];
        let dir = tempdir().expect("Cannot create temp directory");
        let file_path = dir.path().join("test-object-file.txt");
        let mut file = File::create(&file_path).expect("Cannot create temporary file");
        file.write_all(object_content.as_bytes())
            .expect("Cannot write into temporary file");

        let expected_bucket_object = BucketObject {
            bucket_name: bucket_name.to_string(),
            key: object_key.to_string(),
            value: object_content.as_bytes().to_vec(),
            tags: object_tags.clone(),
        };

        let mut service_mock = BlobStorageService::faux();
        faux::when!(
            service_mock.put_object(
                _,
                bucket_name,
                object_key,
                _,
                Some(HashMap::from_iter(
                    object_tags
                        .iter()
                        .map(|tag| {
                            let mut parts = tag.split('=');
                            let key = parts.next().unwrap_or_default();
                            let value = parts.next().unwrap_or_default();
                            (key.to_string(), value.to_string())
                        })
                        .collect::<Vec<(String, String)>>()
                ))
            )
        )
        .then_return(Ok(expected_bucket_object.clone()));

        let blob_storage = AzureOS::new("123", uuid::Uuid::new_v4(), "test_123", std::sync::Arc::new(service_mock));

        // execute:
        let added_object = blob_storage
            .put_object(&storage_account, bucket_name, object_key, &file_path, Some(object_tags))
            .expect("Cannot get object from bucket");

        // verify:
        assert_eq!(expected_bucket_object, added_object);
    }

    #[test]
    fn put_object_failure_test() {
        // setup:
        let bucket_name = "test-bucket";
        let storage_account = StorageAccount {
            account_name: "account_123".to_string(),
            access_key: "access_key_123".to_string(),
        };
        let object_key = "test-object-key";
        let object_content = "test-object-content";
        let object_tags = vec!["tag1=value1".to_string(), "tag2=value2".to_string()];
        let raw_error_message = "put error message";
        let dir = tempdir().expect("Cannot create temp directory");
        let file_path = dir.path().join("test-object-file.txt");
        let mut file = File::create(&file_path).expect("Cannot create temporary file");
        file.write_all(object_content.as_bytes())
            .expect("Cannot write into temporary file");

        let mut service_mock = BlobStorageService::faux();
        faux::when!(
            service_mock.put_object(
                _,
                bucket_name,
                object_key,
                _,
                Some(HashMap::from_iter(
                    object_tags
                        .iter()
                        .map(|tag| {
                            let mut parts = tag.split('=');
                            let key = parts.next().unwrap_or_default();
                            let value = parts.next().unwrap_or_default();
                            (key.to_string(), value.to_string())
                        })
                        .collect::<Vec<(String, String)>>()
                ))
            )
        )
        .then_return(Err(BlobStorageServiceError::CannotPutObjectToBucket {
            storage_account_name: storage_account.account_name.to_string(),
            bucket_name: bucket_name.to_string(),
            object_key: object_key.to_string(),
            raw_error_message: raw_error_message.to_string(),
        }));

        let blob_storage = AzureOS::new("123", uuid::Uuid::new_v4(), "test_123", std::sync::Arc::new(service_mock));

        // execute:
        let added_object =
            blob_storage.put_object(&storage_account, bucket_name, object_key, &file_path, Some(object_tags));

        // verify:
        assert_eq!(
            ObjectStorageError::CannotUploadFile {
                bucket_name: bucket_name.to_string(),
                object_name: object_key.to_string(),
                raw_error_message: format!(
                    "Cannot put object `{}` to bucket `{}` in storage account `{}`: \"{}\"",
                    object_key, bucket_name, storage_account.account_name, raw_error_message
                ),
            },
            added_object.unwrap_err()
        );
    }

    #[test]
    fn delete_object_success_test() {
        // setup:
        let storage_account = StorageAccount {
            account_name: "account_123".to_string(),
            access_key: "access_key_123".to_string(),
        };
        let bucket_name = "test-bucket";
        let object_key = "test-object-key";
        let object_content = "test-object-content";
        let expected_bucket_object = BucketObject {
            bucket_name: bucket_name.to_string(),
            key: object_key.to_string(),
            value: object_content.as_bytes().to_vec(),
            tags: vec![],
        };

        let mut service_mock = BlobStorageService::faux();
        faux::when!(service_mock.delete_object(_, bucket_name, object_key)).then_return(Ok(()));
        faux::when!(service_mock.get_object(_, bucket_name, object_key)).then_return(Ok(expected_bucket_object)); // <- object has to be detected there

        let blob_storage = AzureOS::new("123", uuid::Uuid::new_v4(), "test_123", std::sync::Arc::new(service_mock));

        // execute:
        let delete_object_result = blob_storage.delete_object(&storage_account, bucket_name, object_key);

        // verify:
        assert!(delete_object_result.is_ok());
    }

    #[test]
    fn delete_object_failure_test() {
        // setup:
        let storage_account = StorageAccount {
            account_name: "account_123".to_string(),
            access_key: "access_key_123".to_string(),
        };
        let bucket_name = "test-bucket";
        let object_key = "test-object-key";
        let object_content = "test-object-content";
        let expected_bucket_object = BucketObject {
            bucket_name: bucket_name.to_string(),
            key: object_key.to_string(),
            value: object_content.as_bytes().to_vec(),
            tags: vec![],
        };
        let raw_error_message = "delete error message";

        let mut service_mock = BlobStorageService::faux();
        faux::when!(service_mock.delete_object(_, bucket_name, object_key)).then_return(Err(
            BlobStorageServiceError::CannotDeleteObject {
                storage_account_name: storage_account.account_name.to_string(),
                bucket_name: bucket_name.to_string(),
                object_key: object_key.to_string(),
                raw_error_message: raw_error_message.to_string(),
            },
        ));
        faux::when!(service_mock.get_object(_, bucket_name, object_key)).then_return(Ok(expected_bucket_object)); // <- object has to be detected there

        let blob_storage = AzureOS::new("123", uuid::Uuid::new_v4(), "test_123", std::sync::Arc::new(service_mock));

        // execute:
        let delete_object_result = blob_storage.delete_object(&storage_account, bucket_name, object_key);

        // verify:
        assert_eq!(
            ObjectStorageError::CannotDeleteFile {
                bucket_name: bucket_name.to_string(),
                object_name: object_key.to_string(),
                raw_error_message: format!(
                    "Cannot delete object `{}` from bucket `{}` from storage account `{}`: \"{}\"",
                    object_key, bucket_name, storage_account.account_name, raw_error_message
                ),
            },
            delete_object_result.unwrap_err()
        );
    }

    #[test]
    fn get_object_success_test() {
        // setup:
        let storage_account = StorageAccount {
            account_name: "account_123".to_string(),
            access_key: "access_key_123".to_string(),
        };
        let bucket_name = "test-bucket";
        let object_key = "test-object-key";
        let object_content = "test-object-content";
        let expected_bucket_object = BucketObject {
            bucket_name: bucket_name.to_string(),
            key: object_key.to_string(),
            value: object_content.as_bytes().to_vec(),
            tags: vec![],
        };

        let mut service_mock = BlobStorageService::faux();
        faux::when!(service_mock.get_object(_, bucket_name, object_key))
            .then_return(Ok(expected_bucket_object.clone()));

        let blob_storage = AzureOS::new("123", uuid::Uuid::new_v4(), "test_123", std::sync::Arc::new(service_mock));

        // execute:
        let retrieved_object = blob_storage
            .get_object(&storage_account, bucket_name, object_key)
            .expect("Cannot get object from bucket");

        // verify:
        assert_eq!(expected_bucket_object, retrieved_object);
    }

    #[test]
    fn get_object_failure_test() {
        // setup:
        let storage_account = StorageAccount {
            account_name: "account_123".to_string(),
            access_key: "access_key_123".to_string(),
        };
        let bucket_name = "test-bucket";
        let object_key = "test-object-key";
        let raw_error_message = "get error message";

        let mut service_mock = BlobStorageService::faux();
        faux::when!(service_mock.get_object(_, bucket_name, object_key)).then_return(Err(
            BlobStorageServiceError::CannotGetObject {
                storage_account_name: storage_account.account_name.to_string(),
                bucket_name: bucket_name.to_string(),
                object_key: object_key.to_string(),
                raw_error_message: raw_error_message.to_string(),
            },
        ));

        let blob_storage = AzureOS::new("123", uuid::Uuid::new_v4(), "test_123", std::sync::Arc::new(service_mock));

        // execute:
        let retrieved_object = blob_storage.get_object(&storage_account, bucket_name, object_key);

        // verify:
        assert_eq!(
            ObjectStorageError::CannotGetObjectFile {
                bucket_name: bucket_name.to_string(),
                object_name: object_key.to_string(),
                raw_error_message: format!(
                    "Cannot get object `{}` from bucket `{}` from storage account `{}`: \"{}\"",
                    object_key, bucket_name, storage_account.account_name, raw_error_message
                ),
            },
            retrieved_object.unwrap_err()
        );
    }
}

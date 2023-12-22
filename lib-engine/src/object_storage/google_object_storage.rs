use crate::object_storage::errors::ObjectStorageError;
use crate::object_storage::{Bucket, BucketDeleteStrategy, BucketObject};
use crate::object_storage::{Kind, ObjectStorage};
use crate::services::gcp::object_storage_regions::GcpStorageRegion;
use crate::services::gcp::object_storage_service::ObjectStorageService;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

pub struct GoogleOS {
    id: String,
    _long_id: Uuid,
    name: String,
    project_id: String,
    region: GcpStorageRegion,
    service: Arc<ObjectStorageService>,
}

impl GoogleOS {
    pub fn new(
        id: &str,
        long_id: Uuid,
        name: &str,
        project_id: &str,
        region: GcpStorageRegion,
        service: Arc<ObjectStorageService>,
    ) -> GoogleOS {
        Self {
            id: id.to_string(),
            _long_id: long_id,
            name: name.to_string(),
            project_id: project_id.to_string(),
            region,
            service,
        }
    }
}

impl ObjectStorage for GoogleOS {
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

    fn bucket_exists(&self, bucket_name: &str) -> bool {
        self.service.bucket_exists(bucket_name)
    }

    fn create_bucket(
        &self,
        bucket_name: &str,
        bucket_ttl: Option<Duration>,
        bucket_versioning_activated: bool,
    ) -> Result<Bucket, ObjectStorageError> {
        if let Ok(existing_bucket) = self.get_bucket(bucket_name) {
            return Ok(existing_bucket);
        }

        let creation_date: DateTime<Utc> = Utc::now();
        // TODO(benjaminch): Add bucket versioning option
        match self.service.create_bucket(
            self.project_id.as_str(),
            bucket_name,
            self.region.clone(),
            bucket_ttl,
            bucket_versioning_activated,
            Some(HashMap::from([
                // Tags keys rule: Only hyphens (-), underscores (_), lowercase characters, and numbers are allowed.
                // Keys must start with a lowercase character. International characters are allowed.
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

    fn get_bucket(&self, bucket_name: &str) -> Result<Bucket, ObjectStorageError> {
        match self.service.get_bucket(bucket_name) {
            Ok(o) => Ok(o),
            Err(e) => Err(ObjectStorageError::CannotGetBucket {
                bucket_name: bucket_name.to_string(),
                raw_error_message: e.to_string(),
            }),
        }
    }

    fn delete_bucket(
        &self,
        bucket_name: &str,
        bucket_delete_strategy: BucketDeleteStrategy,
    ) -> Result<(), ObjectStorageError> {
        match bucket_delete_strategy {
            BucketDeleteStrategy::HardDelete => {
                self.service
                    .delete_bucket(bucket_name, true)
                    .map_err(|e| ObjectStorageError::CannotDeleteBucket {
                        bucket_name: bucket_name.to_string(),
                        raw_error_message: e.to_string(),
                    })
            }
            BucketDeleteStrategy::Empty => {
                self.service
                    .empty_bucket(bucket_name)
                    .map_err(|e| ObjectStorageError::CannotEmptyBucket {
                        bucket_name: bucket_name.to_string(),
                        raw_error_message: e.to_string(),
                    })
            }
        }
    }

    fn get_object(&self, bucket_name: &str, object_key: &str) -> Result<BucketObject, ObjectStorageError> {
        match self.service.get_object(bucket_name, object_key) {
            Err(e) => Err(ObjectStorageError::CannotGetObjectFile {
                bucket_name: bucket_name.to_string(),
                object_name: object_key.to_string(),
                raw_error_message: e.to_string(),
            }),
            Ok(object) => Ok(object),
        }
    }

    fn put_object(
        &self,
        bucket_name: &str,
        object_key: &str,
        file_path: &Path,
    ) -> Result<BucketObject, ObjectStorageError> {
        let file_content = std::fs::read(file_path).map_err(|e| ObjectStorageError::CannotUploadFile {
            bucket_name: bucket_name.to_string(),
            object_name: object_key.to_string(),
            raw_error_message: e.to_string(),
        })?;

        match self.service.put_object(bucket_name, object_key, file_content) {
            Ok(object) => Ok(object),
            Err(e) => Err(ObjectStorageError::CannotUploadFile {
                bucket_name: bucket_name.to_string(),
                object_name: object_key.to_string(),
                raw_error_message: e.to_string(),
            }),
        }
    }

    fn delete_object(&self, bucket_name: &str, object_key: &str) -> Result<(), ObjectStorageError> {
        // TODO(benjaminch): not optimal since fine grained statuses are not returned, should know if get is error because file doesn't exist or if anything else
        if self.get_object(bucket_name, object_key).is_err() {
            return Ok(());
        }

        self.service
            .delete_object(bucket_name, object_key)
            .map_err(|e| ObjectStorageError::CannotDeleteFile {
                bucket_name: bucket_name.to_string(),
                object_name: object_key.to_string(),
                raw_error_message: e.to_string(),
            })
    }
}

#[cfg(test)]
mod tests {
    use crate::object_storage::errors::ObjectStorageError;
    use crate::object_storage::google_object_storage::GoogleOS;
    use crate::object_storage::{Bucket, BucketDeleteStrategy, BucketObject, BucketRegion, ObjectStorage};
    use crate::services::gcp::object_storage_regions::GcpStorageRegion;
    use crate::services::gcp::object_storage_service::{ObjectStorageService, ObjectStorageServiceError};
    use chrono::Utc;
    use itertools::izip;
    use std::collections::HashMap;
    use std::fs::File;
    use std::io::Write;
    use std::sync::Arc;
    use std::time::Duration;
    use tempfile::tempdir;
    use uuid::Uuid;

    #[test]
    fn bucket_exists_test() {
        // setup:
        let existing_bucket_name = "this-bucket-exists";
        let not_existing_bucket_name = "this-bucket-doesnt-exist";

        let mut service_mock = ObjectStorageService::faux();
        faux::when!(service_mock.bucket_exists(existing_bucket_name)).then_return(true);
        faux::when!(service_mock.bucket_exists(not_existing_bucket_name)).then_return(false);

        let object_storage = GoogleOS::new(
            "123",
            Uuid::new_v4(),
            "test_123",
            "project_123",
            GcpStorageRegion::EuropeWest9,
            Arc::from(service_mock),
        );

        // execute and verify:
        assert!(!object_storage.bucket_exists(not_existing_bucket_name));
        assert!(object_storage.bucket_exists(existing_bucket_name));
    }

    #[test]
    fn create_bucket_success_test() {
        // setup:
        let bucket_region = GcpStorageRegion::EuropeWest9;

        let bucket_name_test_cases = vec!["abc", "abcabc", "ABC_abc"];
        let bucket_ttl_test_cases = vec![
            None,
            Some(Duration::from_secs(60 * 60 * 24)),     // 1 day
            Some(Duration::from_secs(7 * 60 * 60 * 24)), // 7 day
        ];
        let bucket_versioning_test_cases = vec![true, false];

        for (bucket_name, bucket_ttl, bucket_versioning) in
            izip!(bucket_name_test_cases, bucket_ttl_test_cases, bucket_versioning_test_cases)
        {
            let expected_bucket = Bucket {
                name: bucket_name.to_string(),
                ttl: bucket_ttl,
                versioning_activated: bucket_versioning,
                location: BucketRegion::GcpRegion(bucket_region.clone()),
                labels: Some(HashMap::from([
                    ("CreationDate".to_string(), Utc::now().to_rfc3339()),
                    (
                        "Ttl".to_string(),
                        format!("{}", bucket_ttl.map(|ttl| ttl.as_secs()).unwrap_or(0)),
                    ),
                ])),
            };

            let mut service_mock = ObjectStorageService::faux();

            faux::when!(service_mock.create_bucket(
                _, // project_id,
                bucket_name,
                _, // location
                bucket_ttl,
                bucket_versioning,
                _, // labels
            ))
            .then_return(Ok(expected_bucket.clone()));
            faux::when!(service_mock.get_bucket(bucket_name,)).then_return(Err(
                ObjectStorageServiceError::CannotGetBucket {
                    bucket_name: bucket_name.to_string(),
                    raw_error_message: "Bucket doesn't exist".to_string(),
                },
            ));

            let object_storage = GoogleOS::new(
                "123",
                Uuid::new_v4(),
                "test_123",
                "project_123",
                GcpStorageRegion::EuropeWest9,
                Arc::from(service_mock),
            );

            // execute:
            let created_bucket = object_storage
                .create_bucket(bucket_name, bucket_ttl, bucket_versioning)
                .expect("Error creating bucket");

            // verify:
            assert_eq!(expected_bucket, created_bucket);
        }
    }

    #[test]
    fn create_bucket_existing_test() {
        // setup:
        let bucket_region = GcpStorageRegion::EuropeWest9;

        let bucket_name_test_cases = vec!["abc", "abcabc", "ABC_abc"];
        let bucket_ttl_test_cases = vec![
            None,
            Some(Duration::from_secs(60 * 60 * 24)),     // 1 day
            Some(Duration::from_secs(7 * 60 * 60 * 24)), // 7 day
        ];
        let bucket_versioning_test_cases = vec![true, false];

        for (bucket_name, bucket_ttl, bucket_versioning) in
            izip!(bucket_name_test_cases, bucket_ttl_test_cases, bucket_versioning_test_cases)
        {
            let expected_bucket = Bucket {
                name: bucket_name.to_string(),
                ttl: bucket_ttl,
                versioning_activated: bucket_versioning,
                location: BucketRegion::GcpRegion(bucket_region.clone()),
                labels: Some(HashMap::from([
                    ("CreationDate".to_string(), Utc::now().to_rfc3339()),
                    (
                        "Ttl".to_string(),
                        format!("{}", bucket_ttl.map(|ttl| ttl.as_secs()).unwrap_or(0)),
                    ),
                ])),
            };

            let mut service_mock = ObjectStorageService::faux();

            // Make create service returns an error if called, making sure success comes from the get
            faux::when!(service_mock.create_bucket(
                _, // project_id,
                bucket_name,
                _, // location
                bucket_ttl,
                bucket_versioning,
                _, // labels
            ))
            .then_return(Err(ObjectStorageServiceError::CannotCreateBucket {
                bucket_name: bucket_name.to_string(),
                raw_error_message: "Bucket doesn't exist".to_string(),
            }));
            faux::when!(service_mock.get_bucket(bucket_name,)).then_return(Ok(expected_bucket.clone()));

            let object_storage = GoogleOS::new(
                "123",
                Uuid::new_v4(),
                "test_123",
                "project_123",
                GcpStorageRegion::EuropeWest9,
                Arc::from(service_mock),
            );

            // execute:
            let created_bucket = object_storage
                .create_bucket(bucket_name, bucket_ttl, bucket_versioning)
                .expect("Error creating bucket");

            // verify:
            assert_eq!(expected_bucket, created_bucket);
        }
    }

    #[test]
    fn create_bucket_failure_test() {
        // setup:
        let bucket_name = "test-bucket";
        let raw_error_message = "create error message";

        let mut service_mock = ObjectStorageService::faux();
        faux::when!(service_mock.delete_bucket(bucket_name, _)).then_return(Err(
            ObjectStorageServiceError::CannotCreateBucket {
                bucket_name: bucket_name.to_string(),
                raw_error_message: raw_error_message.to_string(),
            },
        ));

        let object_storage = GoogleOS::new(
            "123",
            Uuid::new_v4(),
            "test_123",
            "project_123",
            GcpStorageRegion::EuropeWest9,
            Arc::from(service_mock),
        );

        // execute:
        let deleted_bucket = object_storage.delete_bucket(bucket_name, BucketDeleteStrategy::HardDelete);

        // verify:
        assert_eq!(
            ObjectStorageError::CannotDeleteBucket {
                bucket_name: bucket_name.to_string(),
                raw_error_message: format!("Cannot create bucket `{}`: \"{}\"", bucket_name, raw_error_message),
            },
            deleted_bucket.unwrap_err()
        );
    }

    #[test]
    fn get_bucket_success_test() {
        // setup:
        let bucket_name = "test-bucket";
        let bucket_ttl = Some(Duration::from_secs(7 * 24 * 60 * 60)); // 7 days
        let expected_bucket = Bucket {
            name: bucket_name.to_string(),
            ttl: bucket_ttl,
            versioning_activated: false,
            location: BucketRegion::GcpRegion(GcpStorageRegion::EuropeWest9),
            labels: Some(HashMap::from([
                ("CreationDate".to_string(), Utc::now().to_rfc3339()),
                (
                    "Ttl".to_string(),
                    format!("{}", bucket_ttl.map(|ttl| ttl.as_secs()).unwrap_or(0)),
                ),
            ])),
        };

        let mut service_mock = ObjectStorageService::faux();
        faux::when!(service_mock.get_bucket(bucket_name)).then_return(Ok(expected_bucket.clone()));

        let object_storage = GoogleOS::new(
            "123",
            Uuid::new_v4(),
            "test_123",
            "project_123",
            GcpStorageRegion::EuropeWest9,
            Arc::from(service_mock),
        );

        // execute:
        let retrieved_bucket = object_storage.get_bucket(bucket_name);

        // verify:
        assert_eq!(Ok(expected_bucket), retrieved_bucket);
    }

    #[test]
    fn get_bucket_failure_test() {
        // setup:
        let bucket_name = "test-bucket";
        let raw_error_message = "get error message";

        let mut service_mock = ObjectStorageService::faux();
        faux::when!(service_mock.get_bucket(bucket_name)).then_return(Err(
            ObjectStorageServiceError::CannotGetBucket {
                bucket_name: bucket_name.to_string(),
                raw_error_message: raw_error_message.to_string(),
            },
        ));

        let object_storage = GoogleOS::new(
            "123",
            Uuid::new_v4(),
            "test_123",
            "project_123",
            GcpStorageRegion::EuropeWest9,
            Arc::from(service_mock),
        );

        // execute:
        let retrieved_bucket = object_storage.get_bucket(bucket_name);

        // verify:
        assert_eq!(
            ObjectStorageError::CannotGetBucket {
                bucket_name: bucket_name.to_string(),
                raw_error_message: format!("Cannot get bucket `{}`: \"{}\"", bucket_name, raw_error_message),
            },
            retrieved_bucket.unwrap_err()
        );
    }

    #[test]
    fn delete_bucket_success_test() {
        // setup:
        let bucket_name = "test-bucket";

        let mut service_mock = ObjectStorageService::faux();
        faux::when!(service_mock.delete_bucket(bucket_name, _)).then_return(Ok(()));

        let object_storage = GoogleOS::new(
            "123",
            Uuid::new_v4(),
            "test_123",
            "project_123",
            GcpStorageRegion::EuropeWest9,
            Arc::from(service_mock),
        );

        // execute:
        let delete_result = object_storage.delete_bucket(bucket_name, BucketDeleteStrategy::HardDelete);

        // verify:
        assert_eq!(Ok(()), delete_result);
    }

    #[test]
    fn delete_bucket_failure_test() {
        // setup:
        let bucket_name = "test-bucket";
        let raw_error_message = "delete error message";

        let mut service_mock = ObjectStorageService::faux();
        faux::when!(service_mock.delete_bucket(bucket_name, _)).then_return(Err(
            ObjectStorageServiceError::CannotDeleteBucket {
                bucket_name: bucket_name.to_string(),
                raw_error_message: raw_error_message.to_string(),
            },
        ));

        let object_storage = GoogleOS::new(
            "123",
            Uuid::new_v4(),
            "test_123",
            "project_123",
            GcpStorageRegion::EuropeWest9,
            Arc::from(service_mock),
        );

        // execute:
        let delete_result = object_storage.delete_bucket(bucket_name, BucketDeleteStrategy::HardDelete);

        // verify:
        assert_eq!(
            ObjectStorageError::CannotDeleteBucket {
                bucket_name: bucket_name.to_string(),
                raw_error_message: format!("Cannot delete bucket `{}`: \"{}\"", bucket_name, raw_error_message),
            },
            delete_result.unwrap_err(),
        );
    }

    #[test]
    fn put_object_success_test() {
        // setup:
        let bucket_name = "test-bucket";
        let object_key = "test-object-key";
        let object_content = "test-object-content";
        let dir = tempdir().expect("Cannot create temp directory");
        let file_path = dir.path().join("test-object-file.txt");
        let mut file = File::create(&file_path).expect("Cannot create temporary file");
        file.write_all(object_content.as_bytes())
            .expect("Cannot write into temporary file");

        let expected_bucket_object = BucketObject {
            bucket_name: bucket_name.to_string(),
            key: object_key.to_string(),
            value: object_content.as_bytes().to_vec(),
        };

        let mut service_mock = ObjectStorageService::faux();
        faux::when!(service_mock.put_object(bucket_name, object_key, _))
            .then_return(Ok(expected_bucket_object.clone()));

        let object_storage = GoogleOS::new(
            "123",
            Uuid::new_v4(),
            "test_123",
            "project_123",
            GcpStorageRegion::EuropeWest9,
            Arc::from(service_mock),
        );

        // execute:
        let added_object = object_storage
            .put_object(bucket_name, object_key, &file_path)
            .expect("Cannot get object from bucket");

        // verify:
        assert_eq!(expected_bucket_object, added_object);
    }

    #[test]
    fn put_object_failure_test() {
        // setup:
        let bucket_name = "test-bucket";
        let object_key = "test-object-key";
        let object_content = "test-object-content";
        let raw_error_message = "put error message";

        let dir = tempdir().expect("Cannot create temp directory");
        let file_path = dir.path().join("test-object-file.txt");
        let mut file = File::create(&file_path).expect("Cannot create temporary file");
        file.write_all(object_content.as_bytes())
            .expect("Cannot write into temporary file");

        let mut service_mock = ObjectStorageService::faux();
        faux::when!(service_mock.put_object(bucket_name, object_key, _)).then_return(Err(
            ObjectStorageServiceError::CannotPutObjectToBucket {
                bucket_name: bucket_name.to_string(),
                object_key: object_key.to_string(),
                raw_error_message: raw_error_message.to_string(),
            },
        ));

        let object_storage = GoogleOS::new(
            "123",
            Uuid::new_v4(),
            "test_123",
            "project_123",
            GcpStorageRegion::EuropeWest9,
            Arc::from(service_mock),
        );

        // execute:
        let added_object = object_storage.put_object(bucket_name, object_key, &file_path);

        // verify:
        assert_eq!(
            ObjectStorageError::CannotUploadFile {
                bucket_name: bucket_name.to_string(),
                object_name: object_key.to_string(),
                raw_error_message: format!(
                    "Cannot put object `{}` to bucket `{}`: \"{}\"",
                    object_key, bucket_name, raw_error_message
                ),
            },
            added_object.unwrap_err()
        );
    }

    #[test]
    fn delete_object_success_test() {
        // setup:
        let bucket_name = "test-bucket";
        let object_key = "test-object-key";
        let object_content = "test-object-content";
        let expected_bucket_object = BucketObject {
            bucket_name: bucket_name.to_string(),
            key: object_key.to_string(),
            value: object_content.as_bytes().to_vec(),
        };

        let mut service_mock = ObjectStorageService::faux();
        faux::when!(service_mock.delete_object(bucket_name, object_key)).then_return(Ok(()));
        faux::when!(service_mock.get_object(bucket_name, object_key)).then_return(Ok(expected_bucket_object)); // <- object has to be detected there

        let object_storage = GoogleOS::new(
            "123",
            Uuid::new_v4(),
            "test_123",
            "project_123",
            GcpStorageRegion::EuropeWest9,
            Arc::from(service_mock),
        );

        // execute:
        let delete_object_result = object_storage.delete_object(bucket_name, object_key);

        // verify:
        assert!(delete_object_result.is_ok());
    }

    #[test]
    fn delete_object_failure_test() {
        // setup:
        let bucket_name = "test-bucket";
        let object_key = "test-object-key";
        let object_content = "test-object-content";
        let raw_error_message = "delete error message";
        let expected_bucket_object = BucketObject {
            bucket_name: bucket_name.to_string(),
            key: object_key.to_string(),
            value: object_content.as_bytes().to_vec(),
        };

        let mut service_mock = ObjectStorageService::faux();
        faux::when!(service_mock.delete_object(bucket_name, object_key)).then_return(Err(
            ObjectStorageServiceError::CannotDeleteObject {
                bucket_name: bucket_name.to_string(),
                object_id: object_key.to_string(),
                raw_error_message: raw_error_message.to_string(),
            },
        ));
        faux::when!(service_mock.get_object(bucket_name, object_key)).then_return(Ok(expected_bucket_object)); // <- object has to be detected there

        let object_storage = GoogleOS::new(
            "123",
            Uuid::new_v4(),
            "test_123",
            "project_123",
            GcpStorageRegion::EuropeWest9,
            Arc::from(service_mock),
        );

        // execute:
        let delete_object_result = object_storage.delete_object(bucket_name, object_key);

        // verify:
        assert_eq!(
            ObjectStorageError::CannotDeleteFile {
                bucket_name: bucket_name.to_string(),
                object_name: object_key.to_string(),
                raw_error_message: format!(
                    "Cannot delete object `{}` from bucket `{}`: \"{}\"",
                    object_key, bucket_name, raw_error_message
                ),
            },
            delete_object_result.unwrap_err()
        );
    }

    #[test]
    fn get_object_success_test() {
        // setup:
        let bucket_name = "test-bucket";
        let object_key = "test-object-key";
        let object_content = "test-object-content";
        let expected_bucket_object = BucketObject {
            bucket_name: bucket_name.to_string(),
            key: object_key.to_string(),
            value: object_content.as_bytes().to_vec(),
        };

        let mut service_mock = ObjectStorageService::faux();
        faux::when!(service_mock.get_object(bucket_name, object_key)).then_return(Ok(expected_bucket_object.clone()));

        let object_storage = GoogleOS::new(
            "123",
            Uuid::new_v4(),
            "test_123",
            "project_123",
            GcpStorageRegion::EuropeWest9,
            Arc::from(service_mock),
        );

        // execute:
        let retrieved_object = object_storage
            .get_object(bucket_name, object_key)
            .expect("Cannot get object from bucket");

        // verify:
        assert_eq!(expected_bucket_object, retrieved_object);
    }

    #[test]
    fn get_object_failure_test() {
        // setup:
        let bucket_name = "test-bucket";
        let object_key = "test-object-key";
        let raw_error_message = "get error message";

        let mut service_mock = ObjectStorageService::faux();
        faux::when!(service_mock.get_object(bucket_name, object_key)).then_return(Err(
            ObjectStorageServiceError::CannotGetObject {
                bucket_name: bucket_name.to_string(),
                object_key: object_key.to_string(),
                raw_error_message: raw_error_message.to_string(),
            },
        ));

        let object_storage = GoogleOS::new(
            "123",
            Uuid::new_v4(),
            "test_123",
            "project_123",
            GcpStorageRegion::EuropeWest9,
            Arc::from(service_mock),
        );

        // execute:
        let retrieved_object = object_storage.get_object(bucket_name, object_key);

        // verify:
        assert_eq!(
            ObjectStorageError::CannotGetObjectFile {
                bucket_name: bucket_name.to_string(),
                object_name: object_key.to_string(),
                raw_error_message: format!(
                    "Cannot get object `{}` from bucket `{}`: \"{}\"",
                    object_key, bucket_name, raw_error_message
                ),
            },
            retrieved_object.unwrap_err()
        );
    }
}

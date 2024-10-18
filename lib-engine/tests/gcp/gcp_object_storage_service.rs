use crate::helpers::gcp::{try_parse_json_credentials_from_str, GCP_REGION, GCP_RESOURCE_TTL};
use crate::helpers::gcp::{GCP_STORAGE_API_BUCKET_WRITE_RATE_LIMITER, GCP_STORAGE_API_OBJECT_WRITE_RATE_LIMITER};
use crate::helpers::utilities::FuncTestsSecrets;
use function_name::named;
use qovery_engine::object_storage::{Bucket, BucketObject, BucketRegion};
use qovery_engine::services::gcp::object_storage_regions::GcpStorageRegion;
use qovery_engine::services::gcp::object_storage_service::ObjectStorageService;
use retry::delay::Fibonacci;
use retry::OperationResult;
use std::cmp::max;
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use tracing::error;
use uuid::Uuid;

/// Note those tests might be a bit long because of the write limitations on bucket / objects

struct BucketParams {
    project_id: String,
    bucket_name: String,
    bucket_location: GcpStorageRegion,
    bucket_ttl: Option<Duration>,
    bucket_labels: Option<HashMap<String, String>>,
    bucket_versioning: bool,
}

impl BucketParams {
    /// Check if current bucket params matches google bucket.
    fn matches(&self, bucket: &Bucket, exclude_labels: Option<HashSet<&str>>) -> bool {
        let bucket_location = match &bucket.location {
            BucketRegion::GcpRegion(gcp_location) => gcp_location,
            _ => return false,
        };

        self.bucket_name == bucket.name
            && &self.bucket_location == bucket_location
            && match exclude_labels {
            None => self.bucket_labels == bucket.labels,
            Some(exclusion) => {
                match (&self.bucket_labels, &bucket.labels) {
                    (Some(labels_1), Some(labels_2)) => {
                        let labels_1: HashSet<_> = labels_1.keys().collect();
                        let labels_2: HashSet<_> = labels_2.keys().collect();
                        labels_1.symmetric_difference(&labels_2).all(|l| exclusion.contains(l.as_str()))
                    },
                    _ => false,
                }
            },
        }
        // TTL
        && match (self.bucket_ttl, bucket.ttl) {
            (Some(self_bucket_ttl), Some(bucket_ttl)) => bucket_ttl == max(self_bucket_ttl, Duration::from_secs(24 * 60 * 60)),
            (None, None) => true,
            _ => false,
        }
        // -> Add new fields here
    }
}

#[cfg(feature = "test-gcp-minimal")]
#[named]
#[test]
fn test_bucket_exists() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let credentials = try_parse_json_credentials_from_str(
        secrets
            .GCP_CREDENTIALS
            .as_ref()
            .expect("GCP_CREDENTIALS is not set in secrets"),
    )
    .expect("Cannot parse GCP_CREDENTIALS");
    let service = ObjectStorageService::new(
        credentials,
        Some(GCP_STORAGE_API_BUCKET_WRITE_RATE_LIMITER.clone()),
        Some(GCP_STORAGE_API_OBJECT_WRITE_RATE_LIMITER.clone()),
    )
    .expect("Cannot initialize google object storage service");

    // create a bucket for the test
    let existing_bucket_name = service
        .create_bucket(
            secrets
                .GCP_PROJECT_NAME
                .expect("GCP_PROJECT_NAME should be defined in secrets")
                .as_str(),
            format!("test-bucket-{}", Uuid::new_v4()).as_str(),
            GcpStorageRegion::from(GCP_REGION),
            Some(*GCP_RESOURCE_TTL),
            false,
            Some(HashMap::from([("test_name".to_string(), function_name!().to_string())])),
        )
        .expect("Cannot create bucket")
        .name;
    // stick a guard on the bucket to delete bucket after test
    let _existing_bucket_name_guard = scopeguard::guard(&existing_bucket_name, |bucket_name| {
        // make sure to delete the bucket after test
        service
            .delete_bucket(bucket_name.as_str(), true)
            .unwrap_or_else(|_| panic!("Cannot delete test bucket `{}` after test", bucket_name));
    });
    let not_existing_bucket_name = format!("{}-not-existing", existing_bucket_name);

    // execute & verify:
    assert!(service.bucket_exists(existing_bucket_name.as_str()));
    assert!(!service.bucket_exists(not_existing_bucket_name.as_str()));
}

#[cfg(feature = "test-gcp-minimal")]
#[test]
#[named]
fn test_get_bucket() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let credentials = try_parse_json_credentials_from_str(
        secrets
            .GCP_CREDENTIALS
            .as_ref()
            .expect("GCP_CREDENTIALS is not set in secrets"),
    )
    .expect("Cannot parse GCP_CREDENTIALS");
    let service = ObjectStorageService::new(
        credentials,
        Some(GCP_STORAGE_API_BUCKET_WRITE_RATE_LIMITER.clone()),
        Some(GCP_STORAGE_API_OBJECT_WRITE_RATE_LIMITER.clone()),
    )
    .expect("Cannot initialize google object storage service");

    // create a bucket for the test
    let existing_bucket_name = service
        .create_bucket(
            secrets
                .GCP_PROJECT_NAME
                .expect("GCP_PROJECT_NAME should be defined in secrets")
                .as_str(),
            format!("test-bucket-{}", Uuid::new_v4()).as_str(),
            GcpStorageRegion::from(GCP_REGION),
            Some(*GCP_RESOURCE_TTL),
            false,
            Some(HashMap::from([("test_name".to_string(), function_name!().to_string())])),
        )
        .expect("Cannot create bucket")
        .name;
    // stick a guard on the bucket to delete bucket after test
    let _existing_bucket_name_guard = scopeguard::guard(&existing_bucket_name, |bucket_name| {
        // make sure to delete the bucket after test
        service
            .delete_bucket(bucket_name.as_str(), true)
            .unwrap_or_else(|_| panic!("Cannot delete test bucket `{}` after test", bucket_name));
    });
    let not_existing_bucket_name = format!("{}-not-existing", existing_bucket_name);

    // execute & verify:
    assert!(service.get_bucket(existing_bucket_name.as_str()).is_ok());
    assert!(service.get_bucket(not_existing_bucket_name.as_str()).is_err());
}

#[cfg(feature = "test-gcp-minimal")]
#[test]
#[named]
fn test_create_bucket_success() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let credentials = try_parse_json_credentials_from_str(
        secrets
            .GCP_CREDENTIALS
            .as_ref()
            .expect("GCP_CREDENTIALS is not set in secrets"),
    )
    .expect("Cannot parse GCP_CREDENTIALS");
    let service = ObjectStorageService::new(
        credentials,
        Some(GCP_STORAGE_API_BUCKET_WRITE_RATE_LIMITER.clone()),
        Some(GCP_STORAGE_API_OBJECT_WRITE_RATE_LIMITER.clone()),
    )
    .expect("Cannot initialize google object storage service");

    let google_project_id = secrets
        .GCP_PROJECT_NAME
        .expect("GCP_PROJECT_NAME should be defined in secrets");

    struct TestCase<'a> {
        input: BucketParams,
        description: &'a str,
    }

    let test_cases = vec![
        TestCase {
            input: BucketParams {
                project_id: google_project_id.to_string(),
                bucket_name: format!("test-bucket-1-{}", Uuid::new_v4()),
                bucket_location: GcpStorageRegion::EuropeWest9,
                bucket_ttl: Some(Duration::from_secs(7 * 60 * 60 * 24)), // 1 week
                bucket_labels: Some(HashMap::from([
                    ("bucket_name".to_string(), "bucket_1".to_string()),
                    ("test_name".to_string(), function_name!().to_string()),
                ])),
                bucket_versioning: false,
            },
            description: "case 1 - create a simple bucket",
        },
        TestCase {
            input: BucketParams {
                project_id: google_project_id.to_string(),
                bucket_name: format!("test-bucket-2-{}", Uuid::new_v4()),
                bucket_location: GcpStorageRegion::EuropeWest9,
                bucket_ttl: Some(Duration::from_secs(60 * 60)), // 1 hour
                bucket_labels: Some(HashMap::from([
                    ("bucket_name".to_string(), "bucket_2".to_string()),
                    ("test_name".to_string(), function_name!().to_string()),
                ])),
                bucket_versioning: false,
            },
            description: "case 2 - create a simple bucket with TTL < 1 day",
        },
        TestCase {
            input: BucketParams {
                project_id: google_project_id.to_string(),
                bucket_name: format!("test-bucket-3-{}", Uuid::new_v4()),
                bucket_location: GcpStorageRegion::EuropeWest9,
                bucket_ttl: None,
                bucket_labels: Some(HashMap::from([
                    ("bucket_name".to_string(), "bucket_3".to_string()),
                    ("test_name".to_string(), function_name!().to_string()),
                ])),
                bucket_versioning: false,
            },
            description: "case 3 - create a simple bucket without TTL",
        },
        TestCase {
            input: BucketParams {
                project_id: google_project_id,
                bucket_name: format!("test-bucket-4-{}", Uuid::new_v4()),
                bucket_location: GcpStorageRegion::EuropeWest9,
                bucket_ttl: None,
                bucket_labels: Some(HashMap::from([
                    ("bucket_name".to_string(), "bucket_4".to_string()),
                    ("test_name".to_string(), function_name!().to_string()),
                ])),
                bucket_versioning: true,
            },
            description: "case 4 - create a simple bucket with versioning",
        },
    ];

    for tc in test_cases {
        // execute:
        let created_bucket = service
            .create_bucket(
                tc.input.project_id.as_str(),
                tc.input.bucket_name.as_str(),
                tc.input.bucket_location.clone(),
                tc.input.bucket_ttl,
                tc.input.bucket_versioning,
                tc.input.bucket_labels.clone(),
            )
            .unwrap_or_else(|_| panic!("Cannot create bucket for test `{}`", tc.description));
        // stick a guard on the bucket to delete bucket after test
        let _created_bucket_guard = scopeguard::guard(&created_bucket, |bucket| {
            // make sure to delete the bucket after test
            service
                .delete_bucket(bucket.name.as_str(), true)
                .unwrap_or_else(|_| panic!("Cannot delete test bucket `{}` after test", &created_bucket.name));
        });

        // verify:
        assert!(tc.input.matches(
            &created_bucket,
            Some(HashSet::from_iter(["ttl", "creation_date"].iter().cloned())) // exclude TTL and creation date as added automatically by the service
        ));
    }
}

#[cfg(feature = "test-gcp-minimal")]
#[test]
#[named]
fn test_update_bucket() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let credentials = try_parse_json_credentials_from_str(
        secrets
            .GCP_CREDENTIALS
            .as_ref()
            .expect("GCP_CREDENTIALS is not set in secrets"),
    )
    .expect("Cannot parse GCP_CREDENTIALS");
    let service = ObjectStorageService::new(
        credentials,
        Some(GCP_STORAGE_API_BUCKET_WRITE_RATE_LIMITER.clone()),
        Some(GCP_STORAGE_API_OBJECT_WRITE_RATE_LIMITER.clone()),
    )
    .expect("Cannot initialize google object storage service");

    // create a bucket for the test
    let existing_bucket = service
        .create_bucket(
            secrets
                .GCP_PROJECT_NAME
                .expect("GCP_PROJECT_NAME should be defined in secrets")
                .as_str(),
            format!("test-bucket-{}", Uuid::new_v4()).as_str(),
            GcpStorageRegion::from(GCP_REGION),
            Some(*GCP_RESOURCE_TTL),
            false,
            Some(HashMap::from([("test_name".to_string(), function_name!().to_string())])),
        )
        .expect("Cannot create bucket");
    // stick a guard on the bucket to delete bucket after test
    let _existing_bucket_name_guard = scopeguard::guard(&existing_bucket.name, |bucket_name| {
        // make sure to delete the bucket after test
        service
            .delete_bucket(bucket_name.as_str(), true)
            .unwrap_or_else(|_| panic!("Cannot delete test bucket `{}` after test", bucket_name));
    });

    // Bucket versioning
    for versioning in [true, false].iter() {
        // execute:
        match service.update_bucket(existing_bucket.name.as_str(), *versioning) {
            // verify:
            Ok(updated_bucket_result) => assert_eq!(versioning, &updated_bucket_result.versioning_activated),
            Err(e) => panic!("Cannot update bucket versioning: {}", e),
        }
    }
}

#[cfg(feature = "test-gcp-minimal")]
#[test]
#[named]
fn test_delete_bucket_using_run_job() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let credentials = try_parse_json_credentials_from_str(
        secrets
            .GCP_CREDENTIALS
            .as_ref()
            .expect("GCP_CREDENTIALS is not set in secrets"),
    )
    .expect("Cannot parse GCP_CREDENTIALS");
    let service = ObjectStorageService::new(
        credentials,
        Some(GCP_STORAGE_API_BUCKET_WRITE_RATE_LIMITER.clone()),
        Some(GCP_STORAGE_API_OBJECT_WRITE_RATE_LIMITER.clone()),
    )
    .expect("Cannot initialize google object storage service");

    // create a bucket for the test
    let existing_bucket_name = service
        .create_bucket(
            secrets
                .GCP_PROJECT_NAME
                .expect("GCP_PROJECT_NAME should be defined in secrets")
                .as_str(),
            format!("test-bucket-{}", Uuid::new_v4()).as_str(),
            GcpStorageRegion::from(GCP_REGION),
            Some(*GCP_RESOURCE_TTL),
            false,
            Some(HashMap::from([("test_name".to_string(), function_name!().to_string())])),
        )
        .expect("Cannot create bucket")
        .name;
    // stick a guard on the bucket to delete bucket after test
    let _existing_bucket_name_guard = scopeguard::guard(&existing_bucket_name, |bucket_name| {
        // make sure to delete the bucket after test
        service
            .delete_bucket(bucket_name.as_str(), true)
            .unwrap_or_else(|_| error!("Cannot delete test bucket `{}` after test", bucket_name));
    });

    // execute:
    let delete_result = service.delete_bucket_non_blocking(
        existing_bucket_name.as_str(),
        GcpStorageRegion::from(GCP_REGION),
        Some(*GCP_RESOURCE_TTL),
    );

    // verify:
    assert!(delete_result.is_ok());
    // deletion job should be executed immediately, but there is a delay in the bucket deletion while job is being created and executed
    // so we need to wait a bit before checking if the bucket is deleted
    let bucket_exists_result = retry::retry(Fibonacci::from_millis(5000).take(5), || {
        if service.bucket_exists(existing_bucket_name.as_str()) {
            OperationResult::Retry("Bucket still exists")
        } else {
            OperationResult::Ok(())
        }
    });
    assert!(bucket_exists_result.is_ok());
}

#[cfg(feature = "test-gcp-minimal")]
#[test]
#[named]
fn test_delete_bucket_with_objects() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let credentials = try_parse_json_credentials_from_str(
        secrets
            .GCP_CREDENTIALS
            .as_ref()
            .expect("GCP_CREDENTIALS is not set in secrets"),
    )
    .expect("Cannot parse GCP_CREDENTIALS");
    let service = ObjectStorageService::new(
        credentials,
        Some(GCP_STORAGE_API_BUCKET_WRITE_RATE_LIMITER.clone()),
        Some(GCP_STORAGE_API_OBJECT_WRITE_RATE_LIMITER.clone()),
    )
    .expect("Cannot initialize google object storage service");

    // create a bucket for the test
    let existing_bucket_name = service
        .create_bucket(
            secrets
                .GCP_PROJECT_NAME
                .expect("GCP_PROJECT_NAME should be defined in secrets")
                .as_str(),
            format!("test-bucket-{}", Uuid::new_v4()).as_str(),
            GcpStorageRegion::from(GCP_REGION),
            Some(*GCP_RESOURCE_TTL),
            false,
            Some(HashMap::from([("test_name".to_string(), function_name!().to_string())])),
        )
        .expect("Cannot create bucket")
        .name;

    let object_key = format!("uploaded-test-file-{}.txt", Uuid::new_v4());
    let object_content = "FILE_CONTENT".to_string();
    let _uploaded_object = service
        .put_object(
            existing_bucket_name.as_str(),
            object_key.as_str(),
            object_content.clone().into_bytes(),
        )
        .unwrap_or_else(|_| panic!("Cannot put object `{}` to bucket `{}`", &object_key, &existing_bucket_name));

    // execute:
    let delete_result = service.delete_bucket(existing_bucket_name.as_str(), true);

    // verify:
    assert!(delete_result.is_ok());
    assert!(!service.bucket_exists(existing_bucket_name.as_str()));
}

#[cfg(feature = "test-gcp-minimal")]
#[test]
#[named]
fn test_empty_bucket_with_objects() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let credentials = try_parse_json_credentials_from_str(
        secrets
            .GCP_CREDENTIALS
            .as_ref()
            .expect("GCP_CREDENTIALS is not set in secrets"),
    )
    .expect("Cannot parse GCP_CREDENTIALS");
    let service = ObjectStorageService::new(
        credentials,
        Some(GCP_STORAGE_API_BUCKET_WRITE_RATE_LIMITER.clone()),
        Some(GCP_STORAGE_API_OBJECT_WRITE_RATE_LIMITER.clone()),
    )
    .expect("Cannot initialize google object storage service");

    // create a bucket for the test
    let existing_bucket_name = service
        .create_bucket(
            secrets
                .GCP_PROJECT_NAME
                .expect("GCP_PROJECT_NAME should be defined in secrets")
                .as_str(),
            format!("test-bucket-{}", Uuid::new_v4()).as_str(),
            GcpStorageRegion::from(GCP_REGION),
            Some(*GCP_RESOURCE_TTL),
            false,
            Some(HashMap::from([("test_name".to_string(), function_name!().to_string())])),
        )
        .expect("Cannot create bucket")
        .name;
    // stick a guard on the bucket to delete bucket after test
    let _existing_bucket_name_guard = scopeguard::guard(&existing_bucket_name, |bucket_name| {
        // make sure to delete the bucket after test
        service
            .delete_bucket(bucket_name.as_str(), true)
            .unwrap_or_else(|_| panic!("Cannot delete test bucket `{}` after test", bucket_name));
    });

    let object_key = format!("uploaded-test-file-{}.txt", Uuid::new_v4());
    let object_content = "FILE_CONTENT".to_string();
    let _uploaded_object = service
        .put_object(
            existing_bucket_name.as_str(),
            object_key.as_str(),
            object_content.clone().into_bytes(),
        )
        .unwrap_or_else(|_| panic!("Cannot put object `{}` to bucket `{}`", &object_key, &existing_bucket_name));

    // execute:
    service
        .empty_bucket(existing_bucket_name.as_str())
        .unwrap_or_else(|_| panic!("Cannot empty to bucket `{}`", &existing_bucket_name));

    // verify:
    assert!(service
        .list_objects_keys_only(existing_bucket_name.as_str(), None)
        .unwrap_or_else(|_| panic!("Cannot list objects keys from bucket `{}`", &existing_bucket_name))
        .is_empty());
}

#[cfg(feature = "test-gcp-minimal")]
#[test]
#[named]
fn test_list_bucket() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let credentials = try_parse_json_credentials_from_str(
        secrets
            .GCP_CREDENTIALS
            .as_ref()
            .expect("GCP_CREDENTIALS is not set in secrets"),
    )
    .expect("Cannot parse GCP_CREDENTIALS");
    let service = ObjectStorageService::new(
        credentials,
        Some(GCP_STORAGE_API_BUCKET_WRITE_RATE_LIMITER.clone()),
        Some(GCP_STORAGE_API_OBJECT_WRITE_RATE_LIMITER.clone()),
    )
    .expect("Cannot initialize google object storage service");
    let project_id = secrets
        .GCP_PROJECT_NAME
        .expect("GCP_PROJECT_NAME should be defined in secrets");

    // create a bucket for the test
    let existing_bucket_name = service
        .create_bucket(
            project_id.as_str(),
            format!("test-bucket-{}", Uuid::new_v4()).as_str(),
            GcpStorageRegion::from(GCP_REGION),
            Some(*GCP_RESOURCE_TTL),
            false,
            Some(HashMap::from([("test_name".to_string(), function_name!().to_string())])),
        )
        .expect("Cannot create bucket")
        .name;
    // stick a guard on the bucket to delete bucket after test
    let _created_bucket_guard = scopeguard::guard(&existing_bucket_name, |bucket_name| {
        // make sure to delete the bucket after test
        service
            .delete_bucket(bucket_name.as_str(), true)
            .unwrap_or_else(|_| panic!("Cannot delete test bucket `{}` after test", bucket_name));
    });

    // execute:
    let buckets = service
        .list_buckets(project_id.as_str(), None)
        .expect("Cannot list buckets");

    // verify:
    assert!(buckets.iter().any(|b| b.name == existing_bucket_name));
}

#[cfg(feature = "test-gcp-minimal")]
#[test]
#[named]
fn test_list_bucket_from_prefix() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let credentials = try_parse_json_credentials_from_str(
        secrets
            .GCP_CREDENTIALS
            .as_ref()
            .expect("GCP_CREDENTIALS is not set in secrets"),
    )
    .expect("Cannot parse GCP_CREDENTIALS");
    let service = ObjectStorageService::new(
        credentials,
        Some(GCP_STORAGE_API_BUCKET_WRITE_RATE_LIMITER.clone()),
        Some(GCP_STORAGE_API_OBJECT_WRITE_RATE_LIMITER.clone()),
    )
    .expect("Cannot initialize google object storage service");
    let project_id = secrets
        .GCP_PROJECT_NAME
        .expect("GCP_PROJECT_NAME should be defined in secrets");

    // create a bucket for the test
    let bucket_prefix = &Uuid::new_v4().to_string()[..6];
    let existing_bucket_name = service
        .create_bucket(
            project_id.as_str(),
            format!("{}-test-bucket-{}", bucket_prefix, Uuid::new_v4()).as_str(),
            GcpStorageRegion::from(GCP_REGION),
            Some(*GCP_RESOURCE_TTL),
            false,
            Some(HashMap::from([("test_name".to_string(), function_name!().to_string())])),
        )
        .expect("Cannot create bucket")
        .name;
    // stick a guard on the bucket to delete bucket after test
    let _created_bucket_guard = scopeguard::guard(&existing_bucket_name, |bucket_name| {
        // make sure to delete the bucket after test
        service
            .delete_bucket(bucket_name.as_str(), true)
            .unwrap_or_else(|_| panic!("Cannot delete test bucket `{}` after test", bucket_name));
    });

    // execute:
    let buckets = service
        .list_buckets(project_id.as_str(), Some(bucket_prefix))
        .expect("Cannot list buckets");

    // verify:
    assert!(buckets.iter().any(|b| b.name == existing_bucket_name));
}

#[cfg(feature = "test-gcp-minimal")]
#[test]
#[named]
fn test_put_object() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let credentials = try_parse_json_credentials_from_str(
        secrets
            .GCP_CREDENTIALS
            .as_ref()
            .expect("GCP_CREDENTIALS is not set in secrets"),
    )
    .expect("Cannot parse GCP_CREDENTIALS");
    let service = ObjectStorageService::new(
        credentials,
        Some(GCP_STORAGE_API_BUCKET_WRITE_RATE_LIMITER.clone()),
        Some(GCP_STORAGE_API_OBJECT_WRITE_RATE_LIMITER.clone()),
    )
    .expect("Cannot initialize google object storage service");

    // create a bucket for the test
    let existing_bucket_name = service
        .create_bucket(
            secrets
                .GCP_PROJECT_NAME
                .expect("GCP_PROJECT_NAME should be defined in secrets")
                .as_str(),
            format!("test-bucket-{}", Uuid::new_v4()).as_str(),
            GcpStorageRegion::from(GCP_REGION),
            Some(*GCP_RESOURCE_TTL),
            false,
            Some(HashMap::from([("test_name".to_string(), function_name!().to_string())])),
        )
        .expect("Cannot create bucket")
        .name;
    // stick a guard on the bucket to delete bucket after test
    let _existing_bucket_name_guard = scopeguard::guard(&existing_bucket_name, |bucket_name| {
        // make sure to delete the bucket after test
        service
            .delete_bucket(bucket_name.as_str(), true)
            .unwrap_or_else(|_| panic!("Cannot delete test bucket `{}` after test", bucket_name));
    });

    let object_key = format!("uploaded-test-file-{}.txt", Uuid::new_v4());
    let object_content = "FILE_CONTENT".to_string();

    // execute:
    let uploaded_object = service
        .put_object(
            existing_bucket_name.as_str(),
            object_key.as_str(),
            object_content.clone().into_bytes(),
        )
        .unwrap_or_else(|_| panic!("Cannot put object `{}` to bucket `{}`", &object_key, &existing_bucket_name));

    // verify:
    assert_eq!(object_key, uploaded_object.key);
    assert_eq!(object_content.into_bytes(), uploaded_object.value);
}

#[cfg(feature = "test-gcp-minimal")]
#[test]
#[named]
fn test_get_object() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let credentials = try_parse_json_credentials_from_str(
        secrets
            .GCP_CREDENTIALS
            .as_ref()
            .expect("GCP_CREDENTIALS is not set in secrets"),
    )
    .expect("Cannot parse GCP_CREDENTIALS");
    let service = ObjectStorageService::new(
        credentials,
        Some(GCP_STORAGE_API_BUCKET_WRITE_RATE_LIMITER.clone()),
        Some(GCP_STORAGE_API_OBJECT_WRITE_RATE_LIMITER.clone()),
    )
    .expect("Cannot initialize google object storage service");

    // create a bucket for the test
    let existing_bucket_name = service
        .create_bucket(
            secrets
                .GCP_PROJECT_NAME
                .expect("GCP_PROJECT_NAME should be defined in secrets")
                .as_str(),
            format!("test-bucket-{}", Uuid::new_v4()).as_str(),
            GcpStorageRegion::from(GCP_REGION),
            Some(*GCP_RESOURCE_TTL),
            false,
            Some(HashMap::from([("test_name".to_string(), function_name!().to_string())])),
        )
        .expect("Cannot create bucket")
        .name;
    // stick a guard on the bucket to delete bucket after test
    let _existing_bucket_name_guard = scopeguard::guard(&existing_bucket_name, |bucket_name| {
        // make sure to delete the bucket after test
        service
            .delete_bucket(bucket_name.as_str(), true)
            .unwrap_or_else(|_| panic!("Cannot delete test bucket `{}` after test", bucket_name));
    });

    let object_key = format!("uploaded-test-file-{}.txt", Uuid::new_v4());
    let object_content = "FILE_CONTENT".to_string();
    let _uploaded_object = service
        .put_object(
            existing_bucket_name.as_str(),
            object_key.as_str(),
            object_content.clone().into_bytes(),
        )
        .unwrap_or_else(|_| panic!("Cannot put object `{}` to bucket `{}`", &object_key, &existing_bucket_name));

    // execute:
    let retrieved_object = service
        .get_object(existing_bucket_name.as_str(), object_key.as_str())
        .unwrap_or_else(|_| panic!("Cannot get object `{}` from bucket `{}`", &object_key, &existing_bucket_name));

    // verify:
    assert_eq!(object_key, retrieved_object.key);
    assert_eq!(object_content.into_bytes(), retrieved_object.value);
}

#[cfg(feature = "test-gcp-minimal")]
#[test]
#[named]
fn test_list_objects_keys_only() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let credentials = try_parse_json_credentials_from_str(
        secrets
            .GCP_CREDENTIALS
            .as_ref()
            .expect("GCP_CREDENTIALS is not set in secrets"),
    )
    .expect("Cannot parse GCP_CREDENTIALS");
    let service = ObjectStorageService::new(
        credentials,
        Some(GCP_STORAGE_API_BUCKET_WRITE_RATE_LIMITER.clone()),
        Some(GCP_STORAGE_API_OBJECT_WRITE_RATE_LIMITER.clone()),
    )
    .expect("Cannot initialize google object storage service");

    // create a bucket for the test
    let existing_bucket_name = service
        .create_bucket(
            secrets
                .GCP_PROJECT_NAME
                .expect("GCP_PROJECT_NAME should be defined in secrets")
                .as_str(),
            format!("test-bucket-{}", Uuid::new_v4()).as_str(),
            GcpStorageRegion::from(GCP_REGION),
            Some(*GCP_RESOURCE_TTL),
            false,
            Some(HashMap::from([("test_name".to_string(), function_name!().to_string())])),
        )
        .expect("Cannot create bucket")
        .name;
    // stick a guard on the bucket to delete bucket after test
    let _existing_bucket_name_guard = scopeguard::guard(&existing_bucket_name, |bucket_name| {
        // make sure to delete the bucket after test
        service
            .delete_bucket(bucket_name.as_str(), true)
            .unwrap_or_else(|_| panic!("Cannot delete test bucket `{}` after test", bucket_name));
    });

    // create 10 files to put in the bucket
    let object_to_be_created: Vec<BucketObject> = (0..10)
        .map(|i| BucketObject {
            bucket_name: existing_bucket_name.to_string(),
            key: format!("uploaded-test-file-{}.txt", Uuid::new_v4()),
            value: format!("FILE_CONTENT_{}", i).into_bytes(),
            tags: vec![],
        })
        .collect();
    for object_to_be_created in &object_to_be_created {
        let _uploaded_object = service
            .put_object(
                existing_bucket_name.as_str(),
                object_to_be_created.key.as_str(),
                object_to_be_created.value.clone(),
            )
            .unwrap_or_else(|_| {
                panic!(
                    "Cannot put object `{}` to bucket `{}`",
                    &object_to_be_created.key, &existing_bucket_name
                )
            });
    }

    // execute:
    let objects_keys = service
        .list_objects_keys_only(existing_bucket_name.as_str(), None)
        .unwrap_or_else(|_| panic!("Cannot list objects keys from bucket `{}`", &existing_bucket_name));

    // verify:
    assert_eq!(object_to_be_created.len(), objects_keys.len());
    assert!(object_to_be_created.iter().all(|o| objects_keys.contains(&o.key)));
}

#[cfg(feature = "test-gcp-minimal")]
#[test]
#[named]
fn test_list_objects_keys_only_with_prefix() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let credentials = try_parse_json_credentials_from_str(
        secrets
            .GCP_CREDENTIALS
            .as_ref()
            .expect("GCP_CREDENTIALS is not set in secrets"),
    )
    .expect("Cannot parse GCP_CREDENTIALS");
    let service = ObjectStorageService::new(
        credentials,
        Some(GCP_STORAGE_API_BUCKET_WRITE_RATE_LIMITER.clone()),
        Some(GCP_STORAGE_API_OBJECT_WRITE_RATE_LIMITER.clone()),
    )
    .expect("Cannot initialize google object storage service");

    // create a bucket for the test
    let existing_bucket_name = service
        .create_bucket(
            secrets
                .GCP_PROJECT_NAME
                .expect("GCP_PROJECT_NAME should be defined in secrets")
                .as_str(),
            format!("test-bucket-{}", Uuid::new_v4()).as_str(),
            GcpStorageRegion::from(GCP_REGION),
            Some(*GCP_RESOURCE_TTL),
            false,
            Some(HashMap::from([("test_name".to_string(), function_name!().to_string())])),
        )
        .expect("Cannot create bucket")
        .name;
    // stick a guard on the bucket to delete bucket after test
    let _existing_bucket_name_guard = scopeguard::guard(&existing_bucket_name, |bucket_name| {
        // make sure to delete the bucket after test
        service
            .delete_bucket(bucket_name.as_str(), true)
            .unwrap_or_else(|_| panic!("Cannot delete test bucket `{}` after test", bucket_name));
    });

    // create 10 files to put in the bucket, only 5 are prefixed
    let prefix = "prefixed-";
    let object_to_be_created: Vec<BucketObject> = (0..10)
        .map(|i| BucketObject {
            bucket_name: existing_bucket_name.to_string(),
            key: format!(
                "{}uploaded-test-file-{}.txt",
                match i % 2 {
                    0 => prefix,
                    _ => "",
                },
                Uuid::new_v4()
            ),
            value: format!("FILE_CONTENT_{}", i).into_bytes(),
            tags: vec![],
        })
        .collect();
    for object_to_be_created in &object_to_be_created {
        let _uploaded_object = service
            .put_object(
                existing_bucket_name.as_str(),
                object_to_be_created.key.as_str(),
                object_to_be_created.value.clone(),
            )
            .unwrap_or_else(|_| {
                panic!(
                    "Cannot put object `{}` to bucket `{}`",
                    &object_to_be_created.key, &existing_bucket_name
                )
            });
    }

    // execute:
    let objects_keys = service
        .list_objects_keys_only(existing_bucket_name.as_str(), Some(prefix))
        .unwrap_or_else(|_| panic!("Cannot list objects keys from bucket `{}`", &existing_bucket_name));

    // verify:
    assert_eq!(
        object_to_be_created
            .iter()
            .filter(|o| o.key.starts_with(prefix))
            .count(),
        objects_keys.len()
    );
    assert!(object_to_be_created
        .iter()
        .filter(|o| o.key.starts_with(prefix))
        .all(|o| objects_keys.contains(&o.key)));
}

#[cfg(feature = "test-gcp-minimal")]
#[test]
#[named]
fn test_list_objects() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let credentials = try_parse_json_credentials_from_str(
        secrets
            .GCP_CREDENTIALS
            .as_ref()
            .expect("GCP_CREDENTIALS is not set in secrets"),
    )
    .expect("Cannot parse GCP_CREDENTIALS");
    let service = ObjectStorageService::new(
        credentials,
        Some(GCP_STORAGE_API_BUCKET_WRITE_RATE_LIMITER.clone()),
        Some(GCP_STORAGE_API_OBJECT_WRITE_RATE_LIMITER.clone()),
    )
    .expect("Cannot initialize google object storage service");

    // create a bucket for the test
    let existing_bucket_name = service
        .create_bucket(
            secrets
                .GCP_PROJECT_NAME
                .expect("GCP_PROJECT_NAME should be defined in secrets")
                .as_str(),
            format!("test-bucket-{}", Uuid::new_v4()).as_str(),
            GcpStorageRegion::from(GCP_REGION),
            Some(*GCP_RESOURCE_TTL),
            false,
            Some(HashMap::from([("test_name".to_string(), function_name!().to_string())])),
        )
        .expect("Cannot create bucket")
        .name;
    // stick a guard on the bucket to delete bucket after test
    let _existing_bucket_name_guard = scopeguard::guard(&existing_bucket_name, |bucket_name| {
        // make sure to delete the bucket after test
        service
            .delete_bucket(bucket_name.as_str(), true)
            .unwrap_or_else(|_| panic!("Cannot delete test bucket `{}` after test", bucket_name));
    });

    // create 10 files to put in the bucket
    let object_to_be_created: Vec<BucketObject> = (0..10)
        .map(|i| BucketObject {
            bucket_name: existing_bucket_name.to_string(),
            key: format!("uploaded-test-file-{}.txt", Uuid::new_v4()),
            value: format!("FILE_CONTENT_{}", i).into_bytes(),
            tags: vec![],
        })
        .collect();
    for object_to_be_created in &object_to_be_created {
        let _uploaded_object = service
            .put_object(
                existing_bucket_name.as_str(),
                object_to_be_created.key.as_str(),
                object_to_be_created.value.clone(),
            )
            .unwrap_or_else(|_| {
                panic!(
                    "Cannot put object `{}` to bucket `{}`",
                    &object_to_be_created.key, &existing_bucket_name
                )
            });
    }

    // execute:
    let objects = service
        .list_objects(existing_bucket_name.as_str(), None)
        .unwrap_or_else(|_| panic!("Cannot list objects from bucket `{}`", &existing_bucket_name));

    // verify:
    assert_eq!(object_to_be_created.len(), objects.len());
    assert!(object_to_be_created.iter().all(|o| objects.contains(o)));
}

#[cfg(feature = "test-gcp-minimal")]
#[test]
#[named]
fn test_list_objects_with_prefix() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let credentials = try_parse_json_credentials_from_str(
        secrets
            .GCP_CREDENTIALS
            .as_ref()
            .expect("GCP_CREDENTIALS is not set in secrets"),
    )
    .expect("Cannot parse GCP_CREDENTIALS");
    let service = ObjectStorageService::new(
        credentials,
        Some(GCP_STORAGE_API_BUCKET_WRITE_RATE_LIMITER.clone()),
        Some(GCP_STORAGE_API_OBJECT_WRITE_RATE_LIMITER.clone()),
    )
    .expect("Cannot initialize google object storage service");

    // create a bucket for the test
    let existing_bucket_name = service
        .create_bucket(
            secrets
                .GCP_PROJECT_NAME
                .expect("GCP_PROJECT_NAME should be defined in secrets")
                .as_str(),
            format!("test-bucket-{}", Uuid::new_v4()).as_str(),
            GcpStorageRegion::from(GCP_REGION),
            Some(*GCP_RESOURCE_TTL),
            false,
            Some(HashMap::from([("test_name".to_string(), function_name!().to_string())])),
        )
        .expect("Cannot create bucket")
        .name;
    // stick a guard on the bucket to delete bucket after test
    let _existing_bucket_name_guard = scopeguard::guard(&existing_bucket_name, |bucket_name| {
        // make sure to delete the bucket after test
        service
            .delete_bucket(bucket_name.as_str(), true)
            .unwrap_or_else(|_| panic!("Cannot delete test bucket `{}` after test", bucket_name));
    });

    // create 10 files to put in the bucket
    let prefix = "prefixed-";
    let object_to_be_created: Vec<BucketObject> = (0..10)
        .map(|i| BucketObject {
            bucket_name: existing_bucket_name.to_string(),
            key: format!(
                "{}uploaded-test-file-{}.txt",
                match i % 2 {
                    0 => prefix,
                    _ => "",
                },
                Uuid::new_v4()
            ),
            value: format!("FILE_CONTENT_{}", i).into_bytes(),
            tags: vec![],
        })
        .collect();
    for object_to_be_created in &object_to_be_created {
        let _uploaded_object = service
            .put_object(
                existing_bucket_name.as_str(),
                object_to_be_created.key.as_str(),
                object_to_be_created.value.clone(),
            )
            .unwrap_or_else(|_| {
                panic!(
                    "Cannot put object `{}` to bucket `{}`",
                    &object_to_be_created.key, &existing_bucket_name
                )
            });
    }

    // execute:
    let objects = service
        .list_objects(existing_bucket_name.as_str(), Some(prefix))
        .unwrap_or_else(|_| panic!("Cannot list objects from bucket `{}`", &existing_bucket_name));

    // verify:
    assert_eq!(
        object_to_be_created
            .iter()
            .filter(|o| o.key.starts_with(prefix))
            .count(),
        objects.len()
    );
    assert!(object_to_be_created
        .iter()
        .filter(|o| o.key.starts_with(prefix))
        .all(|o| objects.contains(o)));
}

#[cfg(feature = "test-gcp-minimal")]
#[test]
#[named]
fn test_delete_object() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let credentials = try_parse_json_credentials_from_str(
        secrets
            .GCP_CREDENTIALS
            .as_ref()
            .expect("GCP_CREDENTIALS is not set in secrets"),
    )
    .expect("Cannot parse GCP_CREDENTIALS");
    let service = ObjectStorageService::new(
        credentials,
        Some(GCP_STORAGE_API_BUCKET_WRITE_RATE_LIMITER.clone()),
        Some(GCP_STORAGE_API_OBJECT_WRITE_RATE_LIMITER.clone()),
    )
    .expect("Cannot initialize google object storage service");

    // create a bucket for the test
    let existing_bucket_name = service
        .create_bucket(
            secrets
                .GCP_PROJECT_NAME
                .expect("GCP_PROJECT_NAME should be defined in secrets")
                .as_str(),
            format!("test-bucket-{}", Uuid::new_v4()).as_str(),
            GcpStorageRegion::from(GCP_REGION),
            Some(*GCP_RESOURCE_TTL),
            false,
            Some(HashMap::from([("test_name".to_string(), function_name!().to_string())])),
        )
        .expect("Cannot create bucket")
        .name;
    // stick a guard on the bucket to delete bucket after test
    let _existing_bucket_name_guard = scopeguard::guard(&existing_bucket_name, |bucket_name| {
        // make sure to delete the bucket after test
        service
            .delete_bucket(bucket_name.as_str(), true)
            .unwrap_or_else(|_| panic!("Cannot delete test bucket `{}` after test", bucket_name));
    });

    let object_key = format!("uploaded-test-file-{}.txt", Uuid::new_v4());
    let object_content = "FILE_CONTENT".to_string();
    let _uploaded_object = service
        .put_object(
            existing_bucket_name.as_str(),
            object_key.as_str(),
            object_content.clone().into_bytes(),
        )
        .unwrap_or_else(|_| panic!("Cannot put object `{}` to bucket `{}`", &object_key, &existing_bucket_name));

    // execute:
    service
        .delete_object(existing_bucket_name.as_str(), object_key.as_str())
        .unwrap_or_else(|_| panic!("Cannot delete object `{}` from bucket `{}`", &object_key, &existing_bucket_name));

    // verify:
    assert!(service
        .get_object(existing_bucket_name.as_str(), object_key.as_str())
        .is_err());
}

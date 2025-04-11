use crate::helpers::utilities::FuncTestsSecrets;
use function_name::named;
use qovery_engine::infrastructure::models::object_storage::{Bucket, BucketRegion};
use qovery_engine::services::azure::blob_storage_regions::AzureStorageRegion;
use qovery_engine::services::azure::blob_storage_service::{BlobStorageService, StorageAccount};
use std::cmp::max;
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use uuid::Uuid;

// TODO(benjaminch): to be refactored with GCP one
struct BucketParams {
    bucket_name: String,
    bucket_location: AzureStorageRegion,
    bucket_ttl: Option<Duration>,
    bucket_labels: Option<HashMap<String, String>>,
}

impl BucketParams {
    /// Check if current bucket params matches google bucket.
    fn matches(&self, bucket: &Bucket, exclude_labels: Option<HashSet<&str>>) -> bool {
        let bucket_location = match &bucket.location {
            BucketRegion::AzureRegion(azure_location) => azure_location,
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

#[cfg(feature = "test-azure-minimal")]
#[test]
#[named]
fn test_create_bucket_success() {
    // setup:
    let secrets = FuncTestsSecrets::new();

    let storage_account = StorageAccount {
        access_key: secrets
            .AZURE_STORAGE_ACCESS_KEY
            .expect("AZURE_STORAGE_ACCESS_KEY should be set"),
        account_name: secrets
            .AZURE_STORAGE_ACCOUNT
            .expect("AZURE_STORAGE_ACCOUNT should be set"),
    };

    let service = BlobStorageService::new();

    struct TestCase<'a> {
        input: BucketParams,
        description: &'a str,
    }

    let test_cases = vec![
        TestCase {
            input: BucketParams {
                bucket_name: format!("test-bucket-1-{}", Uuid::new_v4()),
                bucket_location: AzureStorageRegion::Public {
                    account: storage_account.account_name.to_string(),
                },
                bucket_ttl: Some(Duration::from_secs(7 * 60 * 60 * 24)), // 1 week
                bucket_labels: Some(HashMap::from([
                    ("bucket_name".to_string(), "bucket_1".to_string()),
                    ("test_name".to_string(), function_name!().to_string()),
                ])),
            },
            description: "case 1 - create a simple bucket",
        },
        TestCase {
            input: BucketParams {
                bucket_name: format!("test-bucket-2-{}", Uuid::new_v4()),
                bucket_location: AzureStorageRegion::Public {
                    account: storage_account.account_name.to_string(),
                },
                bucket_ttl: Some(Duration::from_secs(60 * 60)), // 1 hour
                bucket_labels: Some(HashMap::from([
                    ("bucket_name".to_string(), "bucket_2".to_string()),
                    ("test_name".to_string(), function_name!().to_string()),
                ])),
            },
            description: "case 2 - create a simple bucket with TTL < 1 day",
        },
        TestCase {
            input: BucketParams {
                bucket_name: format!("test-bucket-3-{}", Uuid::new_v4()),
                bucket_location: AzureStorageRegion::Public {
                    account: storage_account.account_name.to_string(),
                },
                bucket_ttl: None,
                bucket_labels: Some(HashMap::from([
                    ("bucket_name".to_string(), "bucket_3".to_string()),
                    ("test_name".to_string(), function_name!().to_string()),
                ])),
            },
            description: "case 3 - create a simple bucket without TTL",
        },
    ];

    for tc in test_cases {
        // execute:
        let created_bucket = service
            .create_bucket(
                &storage_account,
                tc.input.bucket_name.as_str(),
                &tc.input.bucket_location,
                tc.input.bucket_ttl,
                tc.input.bucket_labels.clone(),
            )
            .unwrap_or_else(|_| panic!("Cannot create bucket `{}` for test", &tc.input.bucket_name));
        // stick a guard on the bucket to delete bucket after test
        let _created_bucket_guard = scopeguard::guard(&created_bucket, |bucket| {
            // make sure to delete the bucket after test
            service
                .delete_bucket(&storage_account, bucket.name.as_str())
                .unwrap_or_else(|_| panic!("Cannot delete test bucket `{}` after test", &created_bucket.name));
        });

        // verify:
        let retrieved_bucket = service
            .get_bucket(&storage_account, tc.input.bucket_name.as_str(), &tc.input.bucket_location)
            .expect("Cannot retrieve created bucket");

        assert!(
            tc.input.matches(
                &retrieved_bucket,
                Some(HashSet::from_iter(["ttl", "creation_date"].iter().cloned())) // exclude TTL and creation date as added automatically by the service
            ),
            "{}",
            tc.description
        );
    }
}

#[cfg(feature = "test-azure-minimal")]
#[test]
#[named]
fn test_bucket_exists() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let storage_account = StorageAccount {
        access_key: secrets
            .AZURE_STORAGE_ACCESS_KEY
            .expect("AZURE_STORAGE_ACCESS_KEY should be set"),
        account_name: secrets
            .AZURE_STORAGE_ACCOUNT
            .expect("AZURE_STORAGE_ACCOUNT should be set"),
    };
    let bucket_location = AzureStorageRegion::Public {
        account: storage_account.account_name.to_string(),
    };

    let service = BlobStorageService::new();

    // creating a bucket
    let existing_bucket = service
        .create_bucket(
            &storage_account,
            format!("test-bucket-{}", Uuid::new_v4()).as_str(),
            &AzureStorageRegion::Public {
                account: storage_account.account_name.to_string(),
            },
            Some(Duration::from_secs(60 * 60)), // 1 hour
            Some(HashMap::from([
                ("bucket_name".to_string(), "bucket_1".to_string()),
                ("test_name".to_string(), function_name!().to_string()),
            ])),
        )
        .unwrap_or_else(|_| panic!("Cannot create bucket for test"));
    // stick a guard on the bucket to delete bucket after test
    let _created_bucket_guard = scopeguard::guard(&existing_bucket, |bucket| {
        // make sure to delete the bucket after test
        service
            .delete_bucket(&storage_account, bucket.name.as_str())
            .unwrap_or_else(|_| panic!("Cannot delete test bucket `{}` after test", &existing_bucket.name));
    });

    let non_existing_bucket_name = format!("{}-ne", existing_bucket.name);

    // execute & verify:
    assert!(service.bucket_exists(&storage_account, existing_bucket.name.as_str(), &bucket_location));
    assert!(!service.bucket_exists(&storage_account, non_existing_bucket_name.as_str(), &bucket_location));
}

#[cfg(feature = "test-azure-minimal")]
#[test]
#[named]
fn test_get_bucket() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let storage_account = StorageAccount {
        access_key: secrets
            .AZURE_STORAGE_ACCESS_KEY
            .expect("AZURE_STORAGE_ACCESS_KEY should be set"),
        account_name: secrets
            .AZURE_STORAGE_ACCOUNT
            .expect("AZURE_STORAGE_ACCOUNT should be set"),
    };
    let bucket_location = AzureStorageRegion::Public {
        account: storage_account.account_name.to_string(),
    };

    let service = BlobStorageService::new();

    // creating a bucket
    let existing_bucket = service
        .create_bucket(
            &storage_account,
            format!("test-bucket-{}", Uuid::new_v4()).as_str(),
            &AzureStorageRegion::Public {
                account: storage_account.account_name.to_string(),
            },
            Some(Duration::from_secs(60 * 60)), // 1 hour
            Some(HashMap::from([
                ("bucket_name".to_string(), "bucket_1".to_string()),
                ("test_name".to_string(), function_name!().to_string()),
            ])),
        )
        .unwrap_or_else(|_| panic!("Cannot create bucket for test"));
    // stick a guard on the bucket to delete bucket after test
    let _created_bucket_guard = scopeguard::guard(&existing_bucket, |bucket| {
        // make sure to delete the bucket after test
        service
            .delete_bucket(&storage_account, bucket.name.as_str())
            .unwrap_or_else(|_| panic!("Cannot delete test bucket `{}` after test", &existing_bucket.name));
    });

    let non_existing_bucket_name = format!("{}-ne", existing_bucket.name);

    // execute & verify:
    let retrieved_bucket = service
        .get_bucket(&storage_account, existing_bucket.name.as_str(), &bucket_location)
        .unwrap_or_else(|_| panic!("Cannot retrieve bucket `{}`", existing_bucket.name));
    assert_eq!(existing_bucket, retrieved_bucket);

    let retrieved_non_existing_bucket =
        service.get_bucket(&storage_account, non_existing_bucket_name.as_str(), &bucket_location);
    assert!(retrieved_non_existing_bucket.is_err());
}

#[cfg(feature = "test-azure-minimal")]
#[test]
#[named]
fn test_delete_bucket() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let storage_account = StorageAccount {
        access_key: secrets
            .AZURE_STORAGE_ACCESS_KEY
            .expect("AZURE_STORAGE_ACCESS_KEY should be set"),
        account_name: secrets
            .AZURE_STORAGE_ACCOUNT
            .expect("AZURE_STORAGE_ACCOUNT should be set"),
    };
    let bucket_location = AzureStorageRegion::Public {
        account: storage_account.account_name.to_string(),
    };

    let service = BlobStorageService::new();

    // creating a bucket
    let existing_bucket = service
        .create_bucket(
            &storage_account,
            format!("test-bucket-{}", Uuid::new_v4()).as_str(),
            &AzureStorageRegion::Public {
                account: storage_account.account_name.to_string(),
            },
            Some(Duration::from_secs(60 * 60)), // 1 hour
            Some(HashMap::from([
                ("bucket_name".to_string(), "bucket_1".to_string()),
                ("test_name".to_string(), function_name!().to_string()),
            ])),
        )
        .unwrap_or_else(|_| panic!("Cannot create bucket for test"));

    // execute:
    let result = service.delete_bucket(&storage_account, existing_bucket.name.as_str());

    // verify:
    assert!(result.is_ok());
    assert!(!service.bucket_exists(&storage_account, existing_bucket.name.as_str(), &bucket_location));
}

#[cfg(feature = "test-azure-minimal")]
#[test]
#[named]
#[ignore = "To be implemented, update is not available on Azure SDK side"]
fn test_update_bucket() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let storage_account = StorageAccount {
        access_key: secrets
            .AZURE_STORAGE_ACCESS_KEY
            .expect("AZURE_STORAGE_ACCESS_KEY should be set"),
        account_name: secrets
            .AZURE_STORAGE_ACCOUNT
            .expect("AZURE_STORAGE_ACCOUNT should be set"),
    };
    let bucket_location = AzureStorageRegion::Public {
        account: storage_account.account_name.to_string(),
    };

    let service = BlobStorageService::new();

    // creating a bucket
    let initial_bucket = Bucket::new(
        format!("test-bucket-{}", Uuid::new_v4()),
        Some(Duration::from_secs(60 * 60)), // 1 hour
        false,
        false,
        BucketRegion::AzureRegion(bucket_location.clone()),
        Some(HashMap::from([
            ("bucket_name".to_string(), "bucket_1".to_string()),
            ("test_name".to_string(), function_name!().to_string()),
        ])),
    );
    let existing_bucket = service
        .create_bucket(
            &storage_account,
            initial_bucket.name.as_str(),
            match initial_bucket.location {
                BucketRegion::AzureRegion(ref azure_location) => azure_location,
                _ => panic!("Invalid bucket location"),
            },
            initial_bucket.ttl,
            Some(HashMap::from([
                ("bucket_name".to_string(), "bucket_1".to_string()),
                ("test_name".to_string(), function_name!().to_string()),
            ])),
        )
        .unwrap_or_else(|_| panic!("Cannot create bucket for test"));
    // stick a guard on the bucket to delete bucket after test
    let existing_bucket_binding = existing_bucket.clone();
    let _created_bucket_guard = scopeguard::guard(&existing_bucket_binding, |bucket| {
        // make sure to delete the bucket after test
        service
            .delete_bucket(&storage_account, bucket.name.as_str())
            .unwrap_or_else(|_| panic!("Cannot delete test bucket `{}` after test", &existing_bucket.name));
    });

    // execute:
    // updating bucket
    let updated_bucket = service
        .update_bucket(
            &storage_account,
            existing_bucket.name.as_str(),
            match existing_bucket.location {
                BucketRegion::AzureRegion(ref azure_location) => azure_location,
                _ => panic!("Invalid bucket location"),
            },
            Some(Duration::from_secs(12 * 60 * 60)), // half day
            false,
            false,
            Some(HashMap::from([
                ("bucket_name".to_string(), "bucket_1".to_string()),
                ("test_name".to_string(), function_name!().to_string()),
                ("updated".to_string(), true.to_string()),
            ])),
        )
        .unwrap_or_else(|_| panic!("Cannot update bucket `{}` for test", &existing_bucket.name));

    // verify:
    assert!(!service.bucket_exists(&storage_account, existing_bucket.name.as_str(), &bucket_location));
    let retrieved_bucket = service
        .get_bucket(&storage_account, updated_bucket.name.as_str(), &bucket_location)
        .expect("Cannot retrieve updated bucket");
    assert_eq!(updated_bucket, retrieved_bucket);
}

#[cfg(feature = "test-azure-minimal")]
#[test]
#[named]
fn test_list_buckets() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let storage_account = StorageAccount {
        access_key: secrets
            .AZURE_STORAGE_ACCESS_KEY
            .expect("AZURE_STORAGE_ACCESS_KEY should be set"),
        account_name: secrets
            .AZURE_STORAGE_ACCOUNT
            .expect("AZURE_STORAGE_ACCOUNT should be set"),
    };
    let bucket_location = AzureStorageRegion::Public {
        account: storage_account.account_name.to_string(),
    };

    let service = BlobStorageService::new();

    // creating a bucket
    let existing_bucket = service
        .create_bucket(
            &storage_account,
            format!("test-bucket-{}", Uuid::new_v4()).as_str(),
            &bucket_location,
            Some(Duration::from_secs(60 * 60)), // 1 hour
            Some(HashMap::from([
                ("bucket_name".to_string(), "bucket_1".to_string()),
                ("test_name".to_string(), function_name!().to_string()),
            ])),
        )
        .unwrap_or_else(|_| panic!("Cannot create bucket for test"));

    // stick a guard on the bucket to delete bucket after test
    let _created_bucket_guard = scopeguard::guard(&existing_bucket, |bucket| {
        // make sure to delete the bucket after test
        service
            .delete_bucket(&storage_account, bucket.name.as_str())
            .unwrap_or_else(|_| panic!("Cannot delete test bucket `{}` after test", &existing_bucket.name));
    });

    // execute:
    let buckets = service
        .list_buckets(&storage_account, &bucket_location, None, None)
        .expect("Cannot list buckets");

    // verify:
    assert!(buckets.iter().any(|b| b.name == existing_bucket.name));
}

#[cfg(feature = "test-azure-minimal")]
#[test]
#[named]
fn test_list_buckets_from_prefix() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let storage_account = StorageAccount {
        access_key: secrets
            .AZURE_STORAGE_ACCESS_KEY
            .expect("AZURE_STORAGE_ACCESS_KEY should be set"),
        account_name: secrets
            .AZURE_STORAGE_ACCOUNT
            .expect("AZURE_STORAGE_ACCOUNT should be set"),
    };
    let bucket_location = AzureStorageRegion::Public {
        account: storage_account.account_name.to_string(),
    };

    let service = BlobStorageService::new();

    // creating a bucket
    let bucket_prefix = &Uuid::new_v4().to_string()[..6];
    let existing_bucket = service
        .create_bucket(
            &storage_account,
            format!("{}-test-bucket-{}", bucket_prefix, Uuid::new_v4()).as_str(),
            &bucket_location,
            Some(Duration::from_secs(60 * 60)), // 1 hour
            Some(HashMap::from([
                ("bucket_name".to_string(), "bucket_1".to_string()),
                ("test_name".to_string(), function_name!().to_string()),
            ])),
        )
        .unwrap_or_else(|_| panic!("Cannot create bucket for test"));

    // stick a guard on the bucket to delete bucket after test
    let _created_bucket_guard = scopeguard::guard(&existing_bucket, |bucket| {
        // make sure to delete the bucket after test
        service
            .delete_bucket(&storage_account, bucket.name.as_str())
            .unwrap_or_else(|_| panic!("Cannot delete test bucket `{}` after test", &existing_bucket.name));
    });

    // execute:
    let buckets = service
        .list_buckets(&storage_account, &bucket_location, Some(bucket_prefix), None)
        .expect("Cannot list buckets");

    // verify:
    assert!(buckets.iter().any(|b| b.name == existing_bucket.name));
}

#[cfg(feature = "test-azure-minimal")]
#[test]
#[named]
fn test_list_buckets_from_metadata() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let storage_account = StorageAccount {
        access_key: secrets
            .AZURE_STORAGE_ACCESS_KEY
            .expect("AZURE_STORAGE_ACCESS_KEY should be set"),
        account_name: secrets
            .AZURE_STORAGE_ACCOUNT
            .expect("AZURE_STORAGE_ACCOUNT should be set"),
    };
    let bucket_location = AzureStorageRegion::Public {
        account: storage_account.account_name.to_string(),
    };

    let service = BlobStorageService::new();

    // creating a bucket
    let bucket_metadata = HashMap::from([
        ("bucket_name".to_string(), "bucket_1".to_string()),
        ("test_name".to_string(), function_name!().to_string()),
    ]);
    let bucket_prefix = &Uuid::new_v4().to_string()[..6];
    let existing_bucket = service
        .create_bucket(
            &storage_account,
            format!("{}-test-bucket-{}", bucket_prefix, Uuid::new_v4()).as_str(),
            &bucket_location,
            Some(Duration::from_secs(60 * 60)), // 1 hour
            Some(bucket_metadata.clone()),
        )
        .unwrap_or_else(|_| panic!("Cannot create bucket for test"));

    // stick a guard on the bucket to delete bucket after test
    let _created_bucket_guard = scopeguard::guard(&existing_bucket, |bucket| {
        // make sure to delete the bucket after test
        service
            .delete_bucket(&storage_account, bucket.name.as_str())
            .unwrap_or_else(|_| panic!("Cannot delete test bucket `{}` after test", &existing_bucket.name));
    });

    // execute:
    let buckets = service
        .list_buckets(&storage_account, &bucket_location, Some(bucket_prefix), Some(bucket_metadata))
        .expect("Cannot list buckets");

    // verify:
    assert!(buckets.iter().any(|b| b.name == existing_bucket.name));
}

#[cfg(feature = "test-azure-minimal")]
#[test]
#[named]
fn test_list_buckets_from_metadata_and_prefix() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let storage_account = StorageAccount {
        access_key: secrets
            .AZURE_STORAGE_ACCESS_KEY
            .expect("AZURE_STORAGE_ACCESS_KEY should be set"),
        account_name: secrets
            .AZURE_STORAGE_ACCOUNT
            .expect("AZURE_STORAGE_ACCOUNT should be set"),
    };
    let bucket_location = AzureStorageRegion::Public {
        account: storage_account.account_name.to_string(),
    };

    let service = BlobStorageService::new();

    // creating a bucket
    let bucket_metadata = HashMap::from([
        ("bucket_name".to_string(), "bucket_1".to_string()),
        ("test_name".to_string(), function_name!().to_string()),
    ]);
    let existing_bucket = service
        .create_bucket(
            &storage_account,
            format!("test-bucket-{}", Uuid::new_v4()).as_str(),
            &bucket_location,
            Some(Duration::from_secs(60 * 60)), // 1 hour
            Some(bucket_metadata.clone()),
        )
        .unwrap_or_else(|_| panic!("Cannot create bucket for test"));

    // stick a guard on the bucket to delete bucket after test
    let _created_bucket_guard = scopeguard::guard(&existing_bucket, |bucket| {
        // make sure to delete the bucket after test
        service
            .delete_bucket(&storage_account, bucket.name.as_str())
            .unwrap_or_else(|_| panic!("Cannot delete test bucket `{}` after test", &existing_bucket.name));
    });

    // execute:
    let buckets = service
        .list_buckets(&storage_account, &bucket_location, None, Some(bucket_metadata))
        .expect("Cannot list buckets");

    // verify:
    assert!(buckets.iter().any(|b| b.name == existing_bucket.name));
}

#[cfg(feature = "test-azure-minimal")]
#[test]
#[named]
fn test_get_object() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let storage_account = StorageAccount {
        access_key: secrets
            .AZURE_STORAGE_ACCESS_KEY
            .expect("AZURE_STORAGE_ACCESS_KEY should be set"),
        account_name: secrets
            .AZURE_STORAGE_ACCOUNT
            .expect("AZURE_STORAGE_ACCOUNT should be set"),
    };
    let bucket_location = AzureStorageRegion::Public {
        account: storage_account.account_name.to_string(),
    };

    let service = BlobStorageService::new();

    // creating a bucket
    let existing_bucket = service
        .create_bucket(
            &storage_account,
            format!("test-bucket-{}", Uuid::new_v4()).as_str(),
            &bucket_location,
            Some(Duration::from_secs(60 * 60)), // 1 hour
            Some(HashMap::from([
                ("bucket_name".to_string(), "bucket_1".to_string()),
                ("test_name".to_string(), function_name!().to_string()),
            ])),
        )
        .unwrap_or_else(|_| panic!("Cannot create bucket for test"));

    // stick a guard on the bucket to delete bucket after test
    let _created_bucket_guard = scopeguard::guard(&existing_bucket, |bucket| {
        // make sure to delete the bucket after test
        service
            .delete_bucket(&storage_account, bucket.name.as_str())
            .unwrap_or_else(|_| panic!("Cannot delete test bucket `{}` after test", &existing_bucket.name));
    });

    // creating an object
    let object_key = format!("uploaded-test-file-{}.txt", Uuid::new_v4());
    let object_content = "FILE_CONTENT".to_string();
    let object_tags = HashMap::from([
        ("bucket_name".to_string(), "bucket_1".to_string()),
        ("test_name".to_string(), function_name!().to_string()),
    ]);
    let _uploaded_object = service
        .put_object(
            &storage_account,
            existing_bucket.name.as_str(),
            object_key.as_str(),
            object_content.clone().into_bytes(),
            Some(object_tags.clone()),
        )
        .unwrap_or_else(|_| panic!("Cannot put object `{}` to bucket `{}`", &object_key, &existing_bucket.name));

    // execute:
    let retrieved_object = service
        .get_object(&storage_account, existing_bucket.name.as_str(), object_key.as_str())
        .unwrap_or_else(|_| panic!("Cannot get object `{}` from bucket `{}`", &object_key, &existing_bucket.name));

    // verify:
    assert_eq!(object_key, retrieved_object.key);
    assert_eq!(object_content.into_bytes(), retrieved_object.value);
    let mut created_object_tags = object_tags
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>();

    let mut object_tags = object_tags
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>();
    object_tags.sort();
    created_object_tags.sort();

    assert_eq!(created_object_tags, object_tags);
}

#[cfg(feature = "test-azure-minimal")]
#[test]
#[named]
fn test_put_object() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let storage_account = StorageAccount {
        access_key: secrets
            .AZURE_STORAGE_ACCESS_KEY
            .expect("AZURE_STORAGE_ACCESS_KEY should be set"),
        account_name: secrets
            .AZURE_STORAGE_ACCOUNT
            .expect("AZURE_STORAGE_ACCOUNT should be set"),
    };
    let bucket_location = AzureStorageRegion::Public {
        account: storage_account.account_name.to_string(),
    };

    let service = BlobStorageService::new();

    // creating a bucket
    let existing_bucket = service
        .create_bucket(
            &storage_account,
            format!("test-bucket-{}", Uuid::new_v4()).as_str(),
            &bucket_location,
            Some(Duration::from_secs(60 * 60)), // 1 hour
            Some(HashMap::from([
                ("bucket_name".to_string(), "bucket_1".to_string()),
                ("test_name".to_string(), function_name!().to_string()),
            ])),
        )
        .unwrap_or_else(|_| panic!("Cannot create bucket for test"));

    // stick a guard on the bucket to delete bucket after test
    let _created_bucket_guard = scopeguard::guard(&existing_bucket, |bucket| {
        // make sure to delete the bucket after test
        service
            .delete_bucket(&storage_account, bucket.name.as_str())
            .unwrap_or_else(|_| panic!("Cannot delete test bucket `{}` after test", &existing_bucket.name));
    });

    // execute:
    let object_key = format!("uploaded-test-file-{}.txt", Uuid::new_v4());
    let object_content = "FILE_CONTENT".to_string();
    let object_tags = HashMap::from([
        ("bucket_name".to_string(), "bucket_1".to_string()),
        ("test_name".to_string(), function_name!().to_string()),
    ]);
    let _uploaded_object = service
        .put_object(
            &storage_account,
            existing_bucket.name.as_str(),
            object_key.as_str(),
            object_content.clone().into_bytes(),
            Some(object_tags.clone()),
        )
        .unwrap_or_else(|_| panic!("Cannot put object `{}` to bucket `{}`", &object_key, &existing_bucket.name));

    // verify:
    let retrieved_object = service
        .get_object(&storage_account, existing_bucket.name.as_str(), object_key.as_str())
        .unwrap_or_else(|_| panic!("Cannot get object `{}` from bucket `{}`", &object_key, &existing_bucket.name));

    assert_eq!(object_key, retrieved_object.key);
    assert_eq!(object_content.into_bytes(), retrieved_object.value);
    let mut created_object_tags = object_tags
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>();
    created_object_tags.sort();
    let mut object_tags = object_tags
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>();
    object_tags.sort();
    assert_eq!(object_tags, created_object_tags);
}

#[cfg(feature = "test-azure-minimal")]
#[test]
#[named]
fn test_list_objects() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let storage_account = StorageAccount {
        access_key: secrets
            .AZURE_STORAGE_ACCESS_KEY
            .expect("AZURE_STORAGE_ACCESS_KEY should be set"),
        account_name: secrets
            .AZURE_STORAGE_ACCOUNT
            .expect("AZURE_STORAGE_ACCOUNT should be set"),
    };
    let bucket_location = AzureStorageRegion::Public {
        account: storage_account.account_name.to_string(),
    };

    let service = BlobStorageService::new();

    // creating a bucket
    let existing_bucket = service
        .create_bucket(
            &storage_account,
            format!("test-bucket-{}", Uuid::new_v4()).as_str(),
            &bucket_location,
            Some(Duration::from_secs(60 * 60)), // 1 hour
            Some(HashMap::from([
                ("bucket_name".to_string(), "bucket_1".to_string()),
                ("test_name".to_string(), function_name!().to_string()),
            ])),
        )
        .unwrap_or_else(|_| panic!("Cannot create bucket for test"));

    // stick a guard on the bucket to delete bucket after test
    let _created_bucket_guard = scopeguard::guard(&existing_bucket, |bucket| {
        // make sure to delete the bucket after test
        service
            .delete_bucket(&storage_account, bucket.name.as_str())
            .unwrap_or_else(|_| panic!("Cannot delete test bucket `{}` after test", &existing_bucket.name));
    });

    let object_keys = [0; 10].map(|_| format!("uploaded-test-file-{}.txt", Uuid::new_v4()));
    for object_key in &object_keys {
        let object_content = "FILE_CONTENT".to_string();
        let object_tags = HashMap::from([
            ("bucket_name".to_string(), "bucket_1".to_string()),
            ("test_name".to_string(), function_name!().to_string()),
        ]);
        let _uploaded_object = service
            .put_object(
                &storage_account,
                existing_bucket.name.as_str(),
                object_key.as_str(),
                object_content.clone().into_bytes(),
                Some(object_tags.clone()),
            )
            .unwrap_or_else(|_| panic!("Cannot put object `{}` to bucket `{}`", &object_key, &existing_bucket.name));
    }

    // execute:
    let objects = service
        .list_objects(&storage_account, existing_bucket.name.as_str(), None, None)
        .expect("Cannot list objects");

    // verify:
    assert_eq!(object_keys.len(), objects.len());
    for object_key in object_keys {
        assert!(objects.iter().any(|o| o.key == object_key));
    }
}

#[cfg(feature = "test-azure-minimal")]
#[test]
#[named]
fn test_list_objects_from_prefix() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let storage_account = StorageAccount {
        access_key: secrets
            .AZURE_STORAGE_ACCESS_KEY
            .expect("AZURE_STORAGE_ACCESS_KEY should be set"),
        account_name: secrets
            .AZURE_STORAGE_ACCOUNT
            .expect("AZURE_STORAGE_ACCOUNT should be set"),
    };
    let bucket_location = AzureStorageRegion::Public {
        account: storage_account.account_name.to_string(),
    };

    let service = BlobStorageService::new();

    // creating a bucket
    let existing_bucket = service
        .create_bucket(
            &storage_account,
            format!("test-bucket-{}", Uuid::new_v4()).as_str(),
            &bucket_location,
            Some(Duration::from_secs(60 * 60)), // 1 hour
            Some(HashMap::from([
                ("bucket_name".to_string(), "bucket_1".to_string()),
                ("test_name".to_string(), function_name!().to_string()),
            ])),
        )
        .unwrap_or_else(|_| panic!("Cannot create bucket for test"));

    // stick a guard on the bucket to delete bucket after test
    let _created_bucket_guard = scopeguard::guard(&existing_bucket, |bucket| {
        // make sure to delete the bucket after test
        service
            .delete_bucket(&storage_account, bucket.name.as_str())
            .unwrap_or_else(|_| panic!("Cannot delete test bucket `{}` after test", &existing_bucket.name));
    });

    let object_key_prefix = &Uuid::new_v4().to_string()[..6];
    let object_keys = [0; 10].map(|_| format!("{}-uploaded-test-file-{}.txt", object_key_prefix, Uuid::new_v4()));
    for object_key in &object_keys {
        let object_content = "FILE_CONTENT".to_string();
        let object_tags = HashMap::from([
            ("bucket_name".to_string(), "bucket_1".to_string()),
            ("test_name".to_string(), function_name!().to_string()),
        ]);
        let _uploaded_object = service
            .put_object(
                &storage_account,
                existing_bucket.name.as_str(),
                object_key.as_str(),
                object_content.clone().into_bytes(),
                Some(object_tags.clone()),
            )
            .unwrap_or_else(|_| panic!("Cannot put object `{}` to bucket `{}`", &object_key, &existing_bucket.name));
    }

    // execute:
    let objects = service
        .list_objects(&storage_account, existing_bucket.name.as_str(), Some(object_key_prefix), None)
        .expect("Cannot list objects");

    // verify:
    assert_eq!(object_keys.len(), objects.len());
    for object_key in object_keys {
        assert!(objects.iter().any(|o| o.key == object_key));
    }
}

#[cfg(feature = "test-azure-minimal")]
#[test]
#[named]
fn test_list_objects_from_metadata() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let storage_account = StorageAccount {
        access_key: secrets
            .AZURE_STORAGE_ACCESS_KEY
            .expect("AZURE_STORAGE_ACCESS_KEY should be set"),
        account_name: secrets
            .AZURE_STORAGE_ACCOUNT
            .expect("AZURE_STORAGE_ACCOUNT should be set"),
    };
    let bucket_location = AzureStorageRegion::Public {
        account: storage_account.account_name.to_string(),
    };

    let service = BlobStorageService::new();

    // creating a bucket
    let existing_bucket = service
        .create_bucket(
            &storage_account,
            format!("test-bucket-{}", Uuid::new_v4()).as_str(),
            &bucket_location,
            Some(Duration::from_secs(60 * 60)), // 1 hour
            Some(HashMap::from([
                ("bucket_name".to_string(), "bucket_1".to_string()),
                ("test_name".to_string(), function_name!().to_string()),
            ])),
        )
        .unwrap_or_else(|_| panic!("Cannot create bucket for test"));

    // stick a guard on the bucket to delete bucket after test
    let _created_bucket_guard = scopeguard::guard(&existing_bucket, |bucket| {
        // make sure to delete the bucket after test
        service
            .delete_bucket(&storage_account, bucket.name.as_str())
            .unwrap_or_else(|_| panic!("Cannot delete test bucket `{}` after test", &existing_bucket.name));
    });

    let object_keys = [0; 10].map(|_| format!("uploaded-test-file-{}.txt", Uuid::new_v4()));
    let object_tags = HashMap::from([
        ("bucket_name".to_string(), "bucket_1".to_string()),
        ("test_name".to_string(), function_name!().to_string()),
    ]);
    for object_key in &object_keys {
        let object_content = "FILE_CONTENT".to_string();

        let _uploaded_object = service
            .put_object(
                &storage_account,
                existing_bucket.name.as_str(),
                object_key.as_str(),
                object_content.clone().into_bytes(),
                Some(object_tags.clone()),
            )
            .unwrap_or_else(|_| panic!("Cannot put object `{}` to bucket `{}`", &object_key, &existing_bucket.name));
    }

    // execute:
    let objects = service
        .list_objects(&storage_account, existing_bucket.name.as_str(), None, Some(object_tags))
        .expect("Cannot list objects");

    // verify:
    assert_eq!(object_keys.len(), objects.len());
    for object_key in object_keys {
        assert!(objects.iter().any(|o| o.key == object_key));
    }
}

#[cfg(feature = "test-azure-minimal")]
#[test]
#[named]
fn test_list_objects_from_prefix_and_metadata() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let storage_account = StorageAccount {
        access_key: secrets
            .AZURE_STORAGE_ACCESS_KEY
            .expect("AZURE_STORAGE_ACCESS_KEY should be set"),
        account_name: secrets
            .AZURE_STORAGE_ACCOUNT
            .expect("AZURE_STORAGE_ACCOUNT should be set"),
    };
    let bucket_location = AzureStorageRegion::Public {
        account: storage_account.account_name.to_string(),
    };

    let service = BlobStorageService::new();

    // creating a bucket
    let existing_bucket = service
        .create_bucket(
            &storage_account,
            format!("test-bucket-{}", Uuid::new_v4()).as_str(),
            &bucket_location,
            Some(Duration::from_secs(60 * 60)), // 1 hour
            Some(HashMap::from([
                ("bucket_name".to_string(), "bucket_1".to_string()),
                ("test_name".to_string(), function_name!().to_string()),
            ])),
        )
        .unwrap_or_else(|_| panic!("Cannot create bucket for test"));

    // stick a guard on the bucket to delete bucket after test
    let _created_bucket_guard = scopeguard::guard(&existing_bucket, |bucket| {
        // make sure to delete the bucket after test
        service
            .delete_bucket(&storage_account, bucket.name.as_str())
            .unwrap_or_else(|_| panic!("Cannot delete test bucket `{}` after test", &existing_bucket.name));
    });

    let object_key_prefix = &Uuid::new_v4().to_string()[..6];
    let object_keys = [0; 10].map(|_| format!("{}-uploaded-test-file-{}.txt", object_key_prefix, Uuid::new_v4()));
    let object_tags = HashMap::from([
        ("bucket_name".to_string(), "bucket_1".to_string()),
        ("test_name".to_string(), function_name!().to_string()),
    ]);
    for object_key in &object_keys {
        let object_content = "FILE_CONTENT".to_string();

        let _uploaded_object = service
            .put_object(
                &storage_account,
                existing_bucket.name.as_str(),
                object_key.as_str(),
                object_content.clone().into_bytes(),
                Some(object_tags.clone()),
            )
            .unwrap_or_else(|_| panic!("Cannot put object `{}` to bucket `{}`", &object_key, &existing_bucket.name));
    }

    // execute:
    let objects = service
        .list_objects(
            &storage_account,
            existing_bucket.name.as_str(),
            Some(object_key_prefix),
            Some(object_tags),
        )
        .expect("Cannot list objects");

    // verify:
    assert_eq!(object_keys.len(), objects.len());
    for object_key in object_keys {
        assert!(objects.iter().any(|o| o.key == object_key));
    }
}

#[cfg(feature = "test-azure-minimal")]
#[test]
#[named]
fn test_delete_object() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let storage_account = StorageAccount {
        access_key: secrets
            .AZURE_STORAGE_ACCESS_KEY
            .expect("AZURE_STORAGE_ACCESS_KEY should be set"),
        account_name: secrets
            .AZURE_STORAGE_ACCOUNT
            .expect("AZURE_STORAGE_ACCOUNT should be set"),
    };
    let bucket_location = AzureStorageRegion::Public {
        account: storage_account.account_name.to_string(),
    };

    let service = BlobStorageService::new();

    // creating a bucket
    let existing_bucket = service
        .create_bucket(
            &storage_account,
            format!("test-bucket-{}", Uuid::new_v4()).as_str(),
            &bucket_location,
            Some(Duration::from_secs(60 * 60)), // 1 hour
            Some(HashMap::from([
                ("bucket_name".to_string(), "bucket_1".to_string()),
                ("test_name".to_string(), function_name!().to_string()),
            ])),
        )
        .unwrap_or_else(|_| panic!("Cannot create bucket for test"));

    // stick a guard on the bucket to delete bucket after test
    let _created_bucket_guard = scopeguard::guard(&existing_bucket, |bucket| {
        // make sure to delete the bucket after test
        service
            .delete_bucket(&storage_account, bucket.name.as_str())
            .unwrap_or_else(|_| panic!("Cannot delete test bucket `{}` after test", &existing_bucket.name));
    });

    // creating an object
    let object_key = format!("uploaded-test-file-{}.txt", Uuid::new_v4());
    let object_content = "FILE_CONTENT".to_string();
    let object_tags = HashMap::from([
        ("bucket_name".to_string(), "bucket_1".to_string()),
        ("test_name".to_string(), function_name!().to_string()),
    ]);
    let _uploaded_object = service
        .put_object(
            &storage_account,
            existing_bucket.name.as_str(),
            object_key.as_str(),
            object_content.clone().into_bytes(),
            Some(object_tags.clone()),
        )
        .unwrap_or_else(|_| panic!("Cannot put object `{}` to bucket `{}`", &object_key, &existing_bucket.name));

    // execute:
    service
        .delete_object(&storage_account, existing_bucket.name.as_str(), object_key.as_str())
        .unwrap_or_else(|_| panic!("Cannot delete object `{}` from bucket `{}`", &object_key, &existing_bucket.name));

    // verify:
    assert!(
        service
            .get_object(&storage_account, existing_bucket.name.as_str(), object_key.as_str())
            .is_err()
    );
}

#[cfg(feature = "test-azure-minimal")]
#[test]
#[named]
fn test_delete_bucket_having_objects() {
    // setup:
    let secrets = FuncTestsSecrets::new();
    let storage_account = StorageAccount {
        access_key: secrets
            .AZURE_STORAGE_ACCESS_KEY
            .expect("AZURE_STORAGE_ACCESS_KEY should be set"),
        account_name: secrets
            .AZURE_STORAGE_ACCOUNT
            .expect("AZURE_STORAGE_ACCOUNT should be set"),
    };
    let bucket_location = AzureStorageRegion::Public {
        account: storage_account.account_name.to_string(),
    };

    let service = BlobStorageService::new();

    // creating a bucket
    let existing_bucket = service
        .create_bucket(
            &storage_account,
            format!("test-bucket-{}", Uuid::new_v4()).as_str(),
            &bucket_location,
            Some(Duration::from_secs(60 * 60)), // 1 hour
            Some(HashMap::from([
                ("bucket_name".to_string(), "bucket_1".to_string()),
                ("test_name".to_string(), function_name!().to_string()),
            ])),
        )
        .unwrap_or_else(|_| panic!("Cannot create bucket for test"));

    // creating an object
    let object_key = format!("uploaded-test-file-{}.txt", Uuid::new_v4());
    let object_content = "FILE_CONTENT".to_string();
    let object_tags = HashMap::from([
        ("bucket_name".to_string(), "bucket_1".to_string()),
        ("test_name".to_string(), function_name!().to_string()),
    ]);
    let _uploaded_object = service
        .put_object(
            &storage_account,
            existing_bucket.name.as_str(),
            object_key.as_str(),
            object_content.clone().into_bytes(),
            Some(object_tags.clone()),
        )
        .unwrap_or_else(|_| panic!("Cannot put object `{}` to bucket `{}`", &object_key, &existing_bucket.name));

    // execute:
    let result = service.delete_bucket(&storage_account, existing_bucket.name.as_str());

    // verify:
    assert!(result.is_ok());
    assert!(!service.bucket_exists(&storage_account, existing_bucket.name.as_str(), &bucket_location));
}

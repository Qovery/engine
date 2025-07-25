use crate::helpers::utilities::{FuncTestsSecrets, engine_run_test, generate_id};
use function_name::named;
use std::time::Duration;

use crate::helpers::scaleway::SCW_BUCKET_TTL_IN_SECONDS;
use qovery_engine::environment::models::scaleway::ScwZone;
use qovery_engine::infrastructure::models::object_storage::scaleway_object_storage::ScalewayOS;
use qovery_engine::infrastructure::models::object_storage::{BucketDeleteStrategy, ObjectStorage};
use tempfile::NamedTempFile;
use tracing::log::info;
use tracing::{Level, span};

// SCW zone has been switched from Paris2 to Warsaw due to a lot of slowness on SCW Object storage end
// making tests very flacky
pub const SCW_OBJECT_STORAGE_TEST_ZONE: ScwZone = ScwZone::Warsaw1;

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn test_delete_bucket_hard_delete_strategy() {
    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();
        // setup:
        let secrets = FuncTestsSecrets::new();
        let scw_access_key = secrets.SCALEWAY_ACCESS_KEY.unwrap_or_else(|| "undefined".to_string());
        let scw_secret_key = secrets.SCALEWAY_SECRET_KEY.unwrap_or_else(|| "undefined".to_string());

        let scaleway_os = ScalewayOS::new(
            generate_id().to_string(),
            "test".to_string(),
            scw_access_key,
            scw_secret_key,
            SCW_OBJECT_STORAGE_TEST_ZONE,
        );

        let bucket_name = format!("qovery-test-bucket-{}", generate_id());

        let create_result = scaleway_os.create_bucket(
            bucket_name.as_str(),
            Some(Duration::from_secs(SCW_BUCKET_TTL_IN_SECONDS)),
            false,
            false,
        );
        assert!(create_result.is_ok());
        info!("Bucket {bucket_name} created.");

        // compute:
        let del_result = scaleway_os.delete_bucket(bucket_name.as_str(), BucketDeleteStrategy::HardDelete);

        // validate:
        assert!(del_result.is_ok());
        assert!(!scaleway_os.bucket_exists(bucket_name.as_str()));
        info!("Bucket {bucket_name} deleted.");

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn test_delete_bucket_empty_strategy() {
    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();
        // setup:
        let secrets = FuncTestsSecrets::new();
        let scw_access_key = secrets.SCALEWAY_ACCESS_KEY.unwrap_or_else(|| "undefined".to_string());
        let scw_secret_key = secrets.SCALEWAY_SECRET_KEY.unwrap_or_else(|| "undefined".to_string());

        let scaleway_os = ScalewayOS::new(
            generate_id().to_string(),
            "test".to_string(),
            scw_access_key,
            scw_secret_key,
            SCW_OBJECT_STORAGE_TEST_ZONE,
        );

        let bucket_name = format!("qovery-test-bucket-{}", generate_id());

        let create_result = scaleway_os.create_bucket(
            bucket_name.as_str(),
            Some(Duration::from_secs(SCW_BUCKET_TTL_IN_SECONDS)),
            false,
            false,
        );
        assert!(create_result.is_ok());
        info!("Bucket {bucket_name} created.");

        // compute:
        let del_result = scaleway_os.delete_bucket(bucket_name.as_str(), BucketDeleteStrategy::Empty);

        // validate:
        assert!(del_result.is_ok());
        assert!(scaleway_os.bucket_exists(bucket_name.as_str()));
        info!("Bucket {bucket_name} not deleted as expected.");

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn test_create_bucket() {
    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();
        // setup:
        let secrets = FuncTestsSecrets::new();
        let scw_access_key = secrets.SCALEWAY_ACCESS_KEY.unwrap_or_else(|| "undefined".to_string());
        let scw_secret_key = secrets.SCALEWAY_SECRET_KEY.unwrap_or_else(|| "undefined".to_string());

        let scaleway_os = ScalewayOS::new(
            generate_id().to_string(),
            "test".to_string(),
            scw_access_key,
            scw_secret_key,
            SCW_OBJECT_STORAGE_TEST_ZONE,
        );

        let bucket_name = format!("qovery-test-bucket-{}", generate_id());

        // compute:
        let result = scaleway_os.create_bucket(
            bucket_name.as_str(),
            Some(Duration::from_secs(SCW_BUCKET_TTL_IN_SECONDS)),
            false,
            false,
        );

        // validate:
        assert!(result.is_ok());
        info!("Bucket {bucket_name} created.");
        assert!(scaleway_os.bucket_exists(bucket_name.as_str()));
        info!("Bucket {bucket_name} exists.");

        // clean-up:
        assert!(
            scaleway_os
                .delete_bucket(bucket_name.as_str(), BucketDeleteStrategy::HardDelete)
                .is_ok()
        );
        info!("Bucket {bucket_name} deleted.");

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn test_get_bucket() {
    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();
        // setup:
        let secrets = FuncTestsSecrets::new();
        let scw_access_key = secrets.SCALEWAY_ACCESS_KEY.unwrap_or_else(|| "undefined".to_string());
        let scw_secret_key = secrets.SCALEWAY_SECRET_KEY.unwrap_or_else(|| "undefined".to_string());

        let scaleway_os = ScalewayOS::new(
            generate_id().to_string(),
            "test".to_string(),
            scw_access_key,
            scw_secret_key,
            SCW_OBJECT_STORAGE_TEST_ZONE,
        );

        let bucket_name = format!("qovery-test-bucket-{}", generate_id());

        let created_bucket = scaleway_os
            .create_bucket(
                bucket_name.as_str(),
                Some(Duration::from_secs(SCW_BUCKET_TTL_IN_SECONDS)),
                false,
                false,
            )
            .expect("Cannot create bucket");

        // compute:
        let retrieved_bucket = scaleway_os
            .get_bucket(bucket_name.as_str())
            .expect("Cannot retrieve bucket");

        // validate:
        assert_eq!(created_bucket, retrieved_bucket);

        // clean-up:
        assert!(
            scaleway_os
                .delete_bucket(bucket_name.as_str(), BucketDeleteStrategy::HardDelete)
                .is_ok()
        );

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn test_recreate_bucket() {
    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        // setup:
        let secrets = FuncTestsSecrets::new();
        let scw_access_key = secrets.SCALEWAY_ACCESS_KEY.unwrap_or_else(|| "undefined".to_string());
        let scw_secret_key = secrets.SCALEWAY_SECRET_KEY.unwrap_or_else(|| "undefined".to_string());

        let scaleway_os = ScalewayOS::new(
            generate_id().to_string(),
            "test".to_string(),
            scw_access_key,
            scw_secret_key,
            SCW_OBJECT_STORAGE_TEST_ZONE,
        );

        let bucket_name = format!("qovery-test-bucket-{}", generate_id());

        // compute & validate:
        let create_result = scaleway_os.create_bucket(
            bucket_name.as_str(),
            Some(Duration::from_secs(SCW_BUCKET_TTL_IN_SECONDS)),
            false,
            false,
        );
        assert!(create_result.is_ok());
        info!("Bucket {bucket_name} created.");
        assert!(scaleway_os.bucket_exists(bucket_name.as_str()));
        info!("Bucket {bucket_name} exists.");

        let delete_result = scaleway_os.delete_bucket(bucket_name.as_str(), BucketDeleteStrategy::HardDelete);
        assert!(delete_result.is_ok());
        info!("Bucket {bucket_name} deleted.");
        assert!(!scaleway_os.bucket_exists(bucket_name.as_str()));

        let recreate_result = scaleway_os.create_bucket(
            bucket_name.as_str(),
            Some(Duration::from_secs(SCW_BUCKET_TTL_IN_SECONDS)),
            false,
            false,
        );
        assert!(recreate_result.is_ok());
        info!("Bucket {bucket_name} recreated.");
        assert!(scaleway_os.bucket_exists(bucket_name.as_str()));
        info!("Bucket {bucket_name} exists again.");

        // clean-up:
        assert!(
            scaleway_os
                .delete_bucket(bucket_name.as_str(), BucketDeleteStrategy::HardDelete,)
                .is_ok()
        );
        info!("Bucket {bucket_name} deleted.");

        test_name.to_string()
    })
}

#[cfg(feature = "test-quarantine")]
#[named]
#[test]
fn test_file_handling() {
    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();
        // setup:
        let secrets = FuncTestsSecrets::new();
        let scw_access_key = secrets.SCALEWAY_ACCESS_KEY.unwrap_or_else(|| "undefined".to_string());
        let scw_secret_key = secrets.SCALEWAY_SECRET_KEY.unwrap_or_else(|| "undefined".to_string());

        let scaleway_os = ScalewayOS::new(
            generate_id().to_string(),
            "test".to_string(),
            scw_access_key,
            scw_secret_key,
            SCW_OBJECT_STORAGE_TEST_ZONE,
        );

        let bucket_name = format!("qovery-test-bucket-{}", generate_id());
        let object_key = format!("test-object-{}", generate_id());

        let create_result = scaleway_os.create_bucket(
            bucket_name.as_str(),
            Some(Duration::from_secs(SCW_BUCKET_TTL_IN_SECONDS)),
            false,
            false,
        );
        assert!(create_result.is_ok());
        info!("Bucket {bucket_name} created.");

        let temp_file = NamedTempFile::new().expect("error while creating tempfile");

        // compute:
        let put_result = scaleway_os.put_object(
            bucket_name.as_str(),
            object_key.as_str(),
            temp_file.into_temp_path().as_ref(),
            None,
        );
        // validate:
        assert!(put_result.is_ok());
        info!("File {object_key} put in bucket {bucket_name}.");

        let get_result = scaleway_os.get_object(bucket_name.as_str(), object_key.as_str());
        assert!(get_result.is_ok());
        info!("File {object_key} get from bucket {bucket_name}.");

        // clean-up:
        assert!(
            scaleway_os
                .delete_bucket(bucket_name.as_str(), BucketDeleteStrategy::HardDelete)
                .is_ok()
        );
        info!("Bucket {bucket_name} deleted.");

        test_name.to_string()
    })
}

#[cfg(feature = "test-quarantine")]
#[named]
#[test]
fn test_ensure_file_is_absent() {
    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();
        // setup:
        let secrets = FuncTestsSecrets::new();
        let scw_access_key = secrets.SCALEWAY_ACCESS_KEY.unwrap_or_else(|| "undefined".to_string());
        let scw_secret_key = secrets.SCALEWAY_SECRET_KEY.unwrap_or_else(|| "undefined".to_string());

        let scaleway_os = ScalewayOS::new(
            generate_id().to_string(),
            "test".to_string(),
            scw_access_key,
            scw_secret_key,
            SCW_OBJECT_STORAGE_TEST_ZONE,
        );

        let bucket_name = format!("qovery-test-bucket-{}", generate_id());
        let object_key = format!("test-object-{}", generate_id());

        let create_result = scaleway_os.create_bucket(
            bucket_name.as_str(),
            Some(Duration::from_secs(SCW_BUCKET_TTL_IN_SECONDS)),
            false,
            false,
        );
        assert!(create_result.is_ok());
        info!("Bucket {bucket_name} created.");

        assert!(scaleway_os.delete_object(&bucket_name, &object_key).is_ok());
        info!("File {object_key} absent from bucket {bucket_name} as expected.");

        let temp_file = NamedTempFile::new().expect("error while creating tempfile");
        let tempfile_path = temp_file.into_temp_path();
        let tempfile_path = tempfile_path.as_ref();

        let put_result = scaleway_os.put_object(bucket_name.as_str(), object_key.as_str(), tempfile_path, None);
        assert!(put_result.is_ok());
        info!("File {object_key} put in bucket {bucket_name}.");

        assert!(scaleway_os.delete_object(&bucket_name, &object_key).is_ok());
        info!("File {object_key} not in bucket {bucket_name} anymore.");

        // clean-up:
        assert!(
            scaleway_os
                .delete_bucket(bucket_name.as_str(), BucketDeleteStrategy::HardDelete)
                .is_ok()
        );
        info!("Bucket {bucket_name} deleted.");

        test_name.to_string()
    })
}

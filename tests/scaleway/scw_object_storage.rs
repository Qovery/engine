use crate::helpers::utilities::{engine_run_test, generate_id, init, FuncTestsSecrets};
use function_name::named;
use std::time::Duration;

use crate::helpers::scaleway::SCW_BUCKET_TTL_IN_SECONDS;
use qovery_engine::models::scaleway::ScwZone;
use qovery_engine::object_storage::scaleway_object_storage::ScalewayOS;
use qovery_engine::object_storage::{BucketDeleteStrategy, ObjectStorage};
use tempfile::NamedTempFile;
use tracing::log::info;
use tracing::{span, Level};

// SCW zone has been switched from Paris2 to Warsaw due to a lot of slowness on SCW Object storage end
// making tests very flacky
pub const SCW_OBJECT_STORAGE_TEST_ZONE: ScwZone = ScwZone::Warsaw1;

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn test_delete_bucket_hard_delete_strategy() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();
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
        );
        assert!(create_result.is_ok());
        info!("Bucket {} created.", bucket_name);

        // compute:
        let del_result = scaleway_os.delete_bucket(bucket_name.as_str(), BucketDeleteStrategy::HardDelete);

        // validate:
        assert!(del_result.is_ok());
        assert!(!scaleway_os.bucket_exists(bucket_name.as_str()));
        info!("Bucket {} deleted.", bucket_name);

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn test_delete_bucket_empty_strategy() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();
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
        );
        assert!(create_result.is_ok());
        info!("Bucket {} created.", bucket_name);

        // compute:
        let del_result = scaleway_os.delete_bucket(bucket_name.as_str(), BucketDeleteStrategy::Empty);

        // validate:
        assert!(del_result.is_ok());
        assert!(scaleway_os.bucket_exists(bucket_name.as_str()));
        info!("Bucket {} not deleted as expected.", bucket_name);

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn test_create_bucket() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();
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
        );

        // validate:
        assert!(result.is_ok());
        info!("Bucket {} created.", bucket_name);
        assert!(scaleway_os.bucket_exists(bucket_name.as_str()));
        info!("Bucket {} exists.", bucket_name);

        // clean-up:
        assert!(scaleway_os
            .delete_bucket(bucket_name.as_str(), BucketDeleteStrategy::HardDelete)
            .is_ok());
        info!("Bucket {} deleted.", bucket_name);

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn test_get_bucket() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();
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
            )
            .expect("Cannot create bucket");

        // compute:
        let retrieved_bucket = scaleway_os
            .get_bucket(bucket_name.as_str())
            .expect("Cannot retrieve bucket");

        // validate:
        assert_eq!(created_bucket, retrieved_bucket);

        // clean-up:
        assert!(scaleway_os
            .delete_bucket(bucket_name.as_str(), BucketDeleteStrategy::HardDelete)
            .is_ok());

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn test_recreate_bucket() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();
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
        );
        assert!(create_result.is_ok());
        info!("Bucket {} created.", bucket_name);
        assert!(scaleway_os.bucket_exists(bucket_name.as_str()));
        info!("Bucket {} exists.", bucket_name);

        let delete_result = scaleway_os.delete_bucket(bucket_name.as_str(), BucketDeleteStrategy::HardDelete);
        assert!(delete_result.is_ok());
        info!("Bucket {} deleted.", bucket_name);
        assert!(!scaleway_os.bucket_exists(bucket_name.as_str()));

        let recreate_result = scaleway_os.create_bucket(
            bucket_name.as_str(),
            Some(Duration::from_secs(SCW_BUCKET_TTL_IN_SECONDS)),
            false,
        );
        assert!(recreate_result.is_ok());
        info!("Bucket {} recreated.", bucket_name);
        assert!(scaleway_os.bucket_exists(bucket_name.as_str()));
        info!("Bucket {} exists again.", bucket_name);

        // clean-up:
        assert!(scaleway_os
            .delete_bucket(bucket_name.as_str(), BucketDeleteStrategy::HardDelete,)
            .is_ok());
        info!("Bucket {} deleted.", bucket_name);

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn test_file_handling() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();
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
        );
        assert!(create_result.is_ok());
        info!("Bucket {} created.", bucket_name);

        let temp_file = NamedTempFile::new().expect("error while creating tempfile");

        // compute:
        let put_result =
            scaleway_os.put_object(bucket_name.as_str(), object_key.as_str(), temp_file.into_temp_path().as_ref());
        // validate:
        assert!(put_result.is_ok());
        info!("File {} put in bucket {}.", object_key, bucket_name);

        let get_result = scaleway_os.get_object(bucket_name.as_str(), object_key.as_str());
        assert!(get_result.is_ok());
        info!("File {} get from bucket {}.", object_key, bucket_name);

        // clean-up:
        assert!(scaleway_os
            .delete_bucket(bucket_name.as_str(), BucketDeleteStrategy::HardDelete)
            .is_ok());
        info!("Bucket {} deleted.", bucket_name);

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn test_ensure_file_is_absent() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();
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
        );
        assert!(create_result.is_ok());
        info!("Bucket {} created.", bucket_name);

        assert!(scaleway_os.delete_object(&bucket_name, &object_key).is_ok());
        info!("File {} absent from bucket {} as expected.", object_key, bucket_name);

        let temp_file = NamedTempFile::new().expect("error while creating tempfile");
        let tempfile_path = temp_file.into_temp_path();
        let tempfile_path = tempfile_path.as_ref();

        let put_result = scaleway_os.put_object(bucket_name.as_str(), object_key.as_str(), tempfile_path);
        assert!(put_result.is_ok());
        info!("File {} put in bucket {}.", object_key, bucket_name);

        assert!(scaleway_os.delete_object(&bucket_name, &object_key).is_ok());
        info!("File {} not in bucket {} anymore.", object_key, bucket_name);

        // clean-up:
        assert!(scaleway_os
            .delete_bucket(bucket_name.as_str(), BucketDeleteStrategy::HardDelete)
            .is_ok());
        info!("Bucket {} deleted.", bucket_name);

        test_name.to_string()
    })
}

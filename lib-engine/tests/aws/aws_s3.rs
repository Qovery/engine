use crate::helpers::utilities::{engine_run_test, generate_id, init, FuncTestsSecrets};
use function_name::named;
use qovery_engine::cloud_provider::aws::regions::AwsRegion;
use qovery_engine::models::ToCloudProviderFormat;
use qovery_engine::object_storage::s3::S3;
use qovery_engine::object_storage::{BucketDeleteStrategy, ObjectStorage};
use retry::delay::Fixed;
use std::str::FromStr;
use std::thread;
use std::time::Duration;
use tempfile::NamedTempFile;
use tracing::{info, span, Level};

#[cfg(feature = "test-aws-minimal")]
#[named]
#[test]
fn test_delete_hard_strategy_bucket() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        // setup:
        let secrets = FuncTestsSecrets::new();
        let id = generate_id();
        let name = format!("test-{id}");
        let aws_access_key = secrets.AWS_ACCESS_KEY_ID.expect("AWS_ACCESS_KEY_ID is not set");
        let aws_secret_key = secrets.AWS_SECRET_ACCESS_KEY.expect("AWS_SECRET_ACCESS_KEY is not set");
        let aws_region_raw = secrets.AWS_DEFAULT_REGION.expect("AWS_DEFAULT_REGION is not set");
        let aws_region = AwsRegion::from_str(aws_region_raw.as_str())
            .unwrap_or_else(|_| panic!("AWS region `{aws_region_raw}` seems not to be valid"));

        let aws_os = S3::new(id.to_string(), name, aws_access_key, aws_secret_key, aws_region.clone());

        let bucket_name = format!("qovery-test-bucket-{}", generate_id());

        aws_os
            .create_bucket(bucket_name.as_str(), Some(Duration::from_secs(7200)), false)
            .unwrap_or_else(|_| {
                panic!("error while creating S3 bucket in `{}`", aws_region.to_cloud_provider_format())
            });

        // compute:
        let result = aws_os.delete_bucket(bucket_name.as_str(), BucketDeleteStrategy::HardDelete);

        // validate:
        assert!(
            result.is_ok(),
            "Delete bucket failed in `{}`",
            aws_region.to_cloud_provider_format()
        );

        // FIXME(benjaminch): wait a bit for bucket deletion to be effective cloud provider side
        thread::sleep(Duration::from_secs(5));

        assert!(
            !aws_os.bucket_exists(bucket_name.as_str()),
            "Delete bucket failed in `{}`, bucket still exists",
            aws_region.to_cloud_provider_format()
        );

        test_name.to_string()
    })
}

#[cfg(feature = "test-aws-minimal")]
#[named]
#[test]
fn test_delete_empty_strategy_bucket() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        // setup:
        let secrets = FuncTestsSecrets::new();
        let id = generate_id();
        let name = format!("test-{id}");
        let aws_access_key = secrets.AWS_ACCESS_KEY_ID.expect("AWS_ACCESS_KEY_ID is not set");
        let aws_secret_key = secrets.AWS_SECRET_ACCESS_KEY.expect("AWS_SECRET_ACCESS_KEY is not set");
        let aws_region_raw = secrets.AWS_DEFAULT_REGION.expect("AWS_DEFAULT_REGION is not set");
        let aws_region = AwsRegion::from_str(aws_region_raw.as_str())
            .unwrap_or_else(|_| panic!("AWS region `{aws_region_raw}` seems not to be valid"));

        let aws_os = S3::new(id.to_string(), name, aws_access_key, aws_secret_key, aws_region.clone());

        let bucket_name = format!("qovery-test-bucket-{}", generate_id());

        aws_os
            .create_bucket(bucket_name.as_str(), Some(Duration::from_secs(7200)), false)
            .unwrap_or_else(|_| {
                panic!("error while creating S3 bucket in `{}`", aws_region.to_cloud_provider_format())
            });

        // compute:
        let result = aws_os.delete_bucket(bucket_name.as_str(), BucketDeleteStrategy::Empty);

        // validate:
        assert!(
            result.is_ok(),
            "Delete bucket failed in `{}`",
            aws_region.to_cloud_provider_format()
        );

        assert!(aws_os.bucket_exists(bucket_name.as_str()),);
        info!("Bucket {} not deleted as expected.", bucket_name);

        test_name.to_string()
    })
}

#[cfg(feature = "test-aws-minimal")]
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
        let id = generate_id();
        let name = format!("test-{id}");
        let aws_access_key = secrets.AWS_ACCESS_KEY_ID.expect("AWS_ACCESS_KEY_ID is not set");
        let aws_secret_key = secrets.AWS_SECRET_ACCESS_KEY.expect("AWS_SECRET_ACCESS_KEY is not set");
        let aws_region_raw = secrets.AWS_DEFAULT_REGION.expect("AWS_DEFAULT_REGION is not set");
        let aws_region = AwsRegion::from_str(aws_region_raw.as_str())
            .unwrap_or_else(|_| panic!("AWS region `{aws_region_raw}` seems not to be valid"));

        let aws_os = S3::new(id.to_string(), name, aws_access_key, aws_secret_key, aws_region.clone());

        let bucket_name = format!("qovery-test-bucket-{}", generate_id());

        // compute:
        let result = aws_os.create_bucket(bucket_name.as_str(), Some(Duration::from_secs(7200)), false);

        // validate:
        assert!(
            result.is_ok(),
            "Create bucket failed in `{}`",
            aws_region.to_cloud_provider_format()
        );
        assert!(
            aws_os.bucket_exists(bucket_name.as_str()),
            "Create bucket failed in `{}`, bucket doesn't exist",
            aws_region.to_cloud_provider_format()
        );

        // clean-up:
        assert!(aws_os
            .delete_bucket(bucket_name.as_str(), BucketDeleteStrategy::HardDelete)
            .is_ok());

        test_name.to_string()
    })
}

#[cfg(feature = "test-aws-minimal")]
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
        let id = generate_id();
        let name = format!("test-{id}");
        let aws_access_key = secrets.AWS_ACCESS_KEY_ID.expect("AWS_ACCESS_KEY_ID is not set");
        let aws_secret_key = secrets.AWS_SECRET_ACCESS_KEY.expect("AWS_SECRET_ACCESS_KEY is not set");
        let aws_region_raw = secrets.AWS_DEFAULT_REGION.expect("AWS_DEFAULT_REGION is not set");
        let aws_region = AwsRegion::from_str(aws_region_raw.as_str())
            .unwrap_or_else(|_| panic!("AWS region `{aws_region_raw}` seems not to be valid"));

        let aws_os = S3::new(id.to_string(), name, aws_access_key, aws_secret_key, aws_region.clone());

        let bucket_name = format!("qovery-test-bucket-{}", generate_id());

        let created_bucket = aws_os
            .create_bucket(bucket_name.as_str(), Some(Duration::from_secs(7200)), false)
            .expect("Cannot create bucket");

        // compute:
        let retrieved_bucket = aws_os.get_bucket(bucket_name.as_str()).expect("Cannot get bucket");

        // validate:
        assert_eq!(created_bucket, retrieved_bucket);

        // clean-up:
        assert!(aws_os
            .delete_bucket(bucket_name.as_str(), BucketDeleteStrategy::HardDelete)
            .is_ok());

        test_name.to_string()
    })
}

#[cfg(feature = "test-aws-minimal")]
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
        let id = generate_id();
        let name = format!("test-{id}");
        let aws_access_key = secrets.AWS_ACCESS_KEY_ID.expect("AWS_ACCESS_KEY_ID is not set");
        let aws_secret_key = secrets.AWS_SECRET_ACCESS_KEY.expect("AWS_SECRET_ACCESS_KEY is not set");
        let aws_region_raw = secrets.AWS_DEFAULT_REGION.expect("AWS_DEFAULT_REGION is not set");
        let aws_region = AwsRegion::from_str(aws_region_raw.as_str())
            .unwrap_or_else(|_| panic!("AWS region `{aws_region_raw}` seems not to be valid"));

        let aws_os = S3::new(id.to_string(), name, aws_access_key, aws_secret_key, aws_region);

        let bucket_name = format!("qovery-test-bucket-{}", generate_id());

        // compute & validate:
        let create_result = aws_os.create_bucket(bucket_name.as_str(), Some(Duration::from_secs(7200)), false);
        assert!(create_result.is_ok());
        assert!(aws_os.bucket_exists(bucket_name.as_str()));

        let delete_result = aws_os.delete_bucket(bucket_name.as_str(), BucketDeleteStrategy::HardDelete);
        assert!(delete_result.is_ok());

        // retry to check if bucket exists, there is a lag / cache after bucket deletion
        assert!(!retry::retry(Fixed::from_millis(1000).take(20), || {
            match aws_os.bucket_exists(bucket_name.as_str()) {
                false => Ok(false),
                true => Err(()),
            }
        })
        .expect("Bucket still exists"));

        let recreate_result = aws_os.create_bucket(bucket_name.as_str(), Some(Duration::from_secs(7200)), false);
        assert!(recreate_result.is_ok());
        // retry to check if bucket exists, there is a lag / cache after bucket deletion
        assert!(retry::retry(Fixed::from_millis(1000).take(20), || {
            match aws_os.bucket_exists(bucket_name.as_str()) {
                true => Ok(true),
                false => Err(()),
            }
        })
        .expect("Bucket doesn't exist"));

        // clean-up:
        assert!(aws_os
            .delete_bucket(bucket_name.as_str(), BucketDeleteStrategy::HardDelete)
            .is_ok());

        test_name.to_string()
    })
}

#[cfg(feature = "test-aws-minimal")]
#[named]
#[test]
fn test_put_file() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        // setup:
        let secrets = FuncTestsSecrets::new();
        let id = generate_id();
        let name = format!("test-{id}");
        let aws_access_key = secrets.AWS_ACCESS_KEY_ID.expect("AWS_ACCESS_KEY_ID is not set");
        let aws_secret_key = secrets.AWS_SECRET_ACCESS_KEY.expect("AWS_SECRET_ACCESS_KEY is not set");
        let aws_region_raw = secrets.AWS_DEFAULT_REGION.expect("AWS_DEFAULT_REGION is not set");
        let aws_region = AwsRegion::from_str(aws_region_raw.as_str())
            .unwrap_or_else(|_| panic!("AWS region `{aws_region_raw}` seems not to be valid"));

        let aws_os = S3::new(id.to_string(), name, aws_access_key, aws_secret_key, aws_region);

        let bucket_name = format!("qovery-test-bucket-{}", generate_id());
        let object_key = format!("test-object-{}", generate_id());

        aws_os
            .create_bucket(bucket_name.as_str(), Some(Duration::from_secs(7200)), false)
            .expect("error while creating object-storage bucket");

        let temp_file = NamedTempFile::new().expect("error while creating tempfile");

        // compute:
        let result = aws_os.put_object(bucket_name.as_str(), object_key.as_str(), temp_file.into_temp_path().as_ref());

        // validate:
        assert!(result.is_ok());
        assert!(aws_os.get_object(bucket_name.as_str(), object_key.as_str()).is_ok());

        // clean-up:
        assert!(aws_os
            .delete_bucket(bucket_name.as_str(), BucketDeleteStrategy::HardDelete)
            .is_ok());

        test_name.to_string()
    })
}

#[cfg(feature = "test-aws-minimal")]
#[named]
#[test]
fn test_get_file() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        // setup:
        let secrets = FuncTestsSecrets::new();
        let id = generate_id();
        let name = format!("test-{id}");
        let aws_access_key = secrets.AWS_ACCESS_KEY_ID.expect("AWS_ACCESS_KEY_ID is not set");
        let aws_secret_key = secrets.AWS_SECRET_ACCESS_KEY.expect("AWS_SECRET_ACCESS_KEY is not set");
        let aws_region_raw = secrets.AWS_DEFAULT_REGION.expect("AWS_DEFAULT_REGION is not set");
        let aws_region = AwsRegion::from_str(aws_region_raw.as_str())
            .unwrap_or_else(|_| panic!("AWS region `{aws_region_raw}` seems not to be valid"));

        let aws_os = S3::new(id.to_string(), name, aws_access_key, aws_secret_key, aws_region);

        let bucket_name = format!("qovery-test-bucket-{}", generate_id());
        let object_key = format!("test-object-{}", generate_id());

        aws_os
            .create_bucket(bucket_name.as_str(), Some(Duration::from_secs(7200)), false)
            .expect("error while creating object-storage bucket");

        let temp_file = NamedTempFile::new().expect("error while creating tempfile");
        let tempfile_path = temp_file.into_temp_path();
        let tempfile_path = tempfile_path.as_ref();

        aws_os
            .put_object(bucket_name.as_str(), object_key.as_str(), tempfile_path)
            .unwrap_or_else(|_| {
                panic!(
                    "error while putting file {} into bucket {bucket_name}",
                    tempfile_path.to_str().unwrap_or_default()
                )
            });

        // compute:
        let result = aws_os.get_object(bucket_name.as_str(), object_key.as_str());

        // validate:
        assert!(result.is_ok());

        // clean-up:
        assert!(aws_os
            .delete_bucket(bucket_name.as_str(), BucketDeleteStrategy::HardDelete)
            .is_ok());

        test_name.to_string()
    })
}

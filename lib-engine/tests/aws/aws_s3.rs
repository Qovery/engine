use crate::helpers::utilities::{context_for_resource, engine_run_test, generate_id, init, FuncTestsSecrets};
use function_name::named;
use qovery_engine::cloud_provider::aws::regions::AwsRegion;
use qovery_engine::object_storage::s3::S3;
use qovery_engine::object_storage::ObjectStorage;
use std::str::FromStr;
use std::time::Duration;
use tempfile::NamedTempFile;
use tracing::{span, Level};
use uuid::Uuid;

#[cfg(feature = "test-aws-minimal")]
#[named]
#[test]
fn test_delete_bucket() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        // setup:
        let context = context_for_resource(Uuid::new_v4(), Uuid::new_v4());
        let secrets = FuncTestsSecrets::new();
        let id = generate_id();
        let name = format!("test-{id}");
        let aws_access_key = secrets.AWS_ACCESS_KEY_ID.expect("AWS_ACCESS_KEY_ID is not set");
        let aws_secret_key = secrets.AWS_SECRET_ACCESS_KEY.expect("AWS_SECRET_ACCESS_KEY is not set");
        let aws_region_raw = secrets.AWS_DEFAULT_REGION.expect("AWS_DEFAULT_REGION is not set");
        let aws_region = AwsRegion::from_str(aws_region_raw.as_str())
            .unwrap_or_else(|_| panic!("AWS region `{aws_region_raw}` seems not to be valid"));

        let aws_os = S3::new(
            context,
            id.to_string(),
            name,
            aws_access_key,
            aws_secret_key,
            aws_region.clone(),
            false,
            Some(Duration::from_secs(7200)),
        );

        let bucket_name = format!("qovery-test-bucket-{}", generate_id());

        aws_os
            .create_bucket(bucket_name.as_str())
            .unwrap_or_else(|_| panic!("error while creating S3 bucket in `{}`", aws_region.to_aws_format()));

        // compute:
        let result = aws_os.delete_bucket(bucket_name.as_str());

        // validate:
        assert!(result.is_ok(), "Delete bucket failed in `{}`", aws_region.to_aws_format());
        assert!(
            !aws_os.bucket_exists(bucket_name.as_str()),
            "Delete bucket failed in `{}`, bucket still exists",
            aws_region.to_aws_format()
        );

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
        let context = context_for_resource(Uuid::new_v4(), Uuid::new_v4());
        let secrets = FuncTestsSecrets::new();
        let id = generate_id();
        let name = format!("test-{id}");
        let aws_access_key = secrets.AWS_ACCESS_KEY_ID.expect("AWS_ACCESS_KEY_ID is not set");
        let aws_secret_key = secrets.AWS_SECRET_ACCESS_KEY.expect("AWS_SECRET_ACCESS_KEY is not set");
        let aws_region_raw = secrets.AWS_DEFAULT_REGION.expect("AWS_DEFAULT_REGION is not set");
        let aws_region = AwsRegion::from_str(aws_region_raw.as_str())
            .unwrap_or_else(|_| panic!("AWS region `{aws_region_raw}` seems not to be valid"));

        let aws_os = S3::new(
            context,
            id.to_string(),
            name,
            aws_access_key,
            aws_secret_key,
            aws_region.clone(),
            false,
            Some(Duration::from_secs(7200)),
        );

        let bucket_name = format!("qovery-test-bucket-{}", generate_id());

        // compute:
        let result = aws_os.create_bucket(bucket_name.as_str());

        // validate:
        assert!(result.is_ok(), "Create bucket failed in `{}`", aws_region.to_aws_format());
        assert!(
            aws_os.bucket_exists(bucket_name.as_str()),
            "Create bucket failed in `{}`, bucket doesn't exist",
            aws_region.to_aws_format()
        );

        // clean-up:
        assert!(aws_os.delete_bucket(bucket_name.as_str()).is_ok());

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
        let context = context_for_resource(Uuid::new_v4(), Uuid::new_v4());
        let secrets = FuncTestsSecrets::new();
        let id = generate_id();
        let name = format!("test-{id}");
        let aws_access_key = secrets.AWS_ACCESS_KEY_ID.expect("AWS_ACCESS_KEY_ID is not set");
        let aws_secret_key = secrets.AWS_SECRET_ACCESS_KEY.expect("AWS_SECRET_ACCESS_KEY is not set");
        let aws_region_raw = secrets.AWS_DEFAULT_REGION.expect("AWS_DEFAULT_REGION is not set");
        let aws_region = AwsRegion::from_str(aws_region_raw.as_str())
            .unwrap_or_else(|_| panic!("AWS region `{aws_region_raw}` seems not to be valid"));

        let aws_os = S3::new(
            context,
            id.to_string(),
            name,
            aws_access_key,
            aws_secret_key,
            aws_region,
            false,
            Some(Duration::from_secs(7200)),
        );

        let bucket_name = format!("qovery-test-bucket-{}", generate_id());

        // compute & validate:
        let create_result = aws_os.create_bucket(bucket_name.as_str());
        assert!(create_result.is_ok());
        assert!(aws_os.bucket_exists(bucket_name.as_str()));

        let delete_result = aws_os.delete_bucket(bucket_name.as_str());
        assert!(delete_result.is_ok());

        let recreate_result = aws_os.create_bucket(bucket_name.as_str());
        assert!(recreate_result.is_ok());
        assert!(aws_os.bucket_exists(bucket_name.as_str()));

        // clean-up:
        assert!(aws_os.delete_bucket(bucket_name.as_str()).is_ok());

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
        let context = context_for_resource(Uuid::new_v4(), Uuid::new_v4());
        let secrets = FuncTestsSecrets::new();
        let id = generate_id();
        let name = format!("test-{id}");
        let aws_access_key = secrets.AWS_ACCESS_KEY_ID.expect("AWS_ACCESS_KEY_ID is not set");
        let aws_secret_key = secrets.AWS_SECRET_ACCESS_KEY.expect("AWS_SECRET_ACCESS_KEY is not set");
        let aws_region_raw = secrets.AWS_DEFAULT_REGION.expect("AWS_DEFAULT_REGION is not set");
        let aws_region = AwsRegion::from_str(aws_region_raw.as_str())
            .unwrap_or_else(|_| panic!("AWS region `{aws_region_raw}` seems not to be valid"));

        let aws_os = S3::new(
            context,
            id.to_string(),
            name,
            aws_access_key,
            aws_secret_key,
            aws_region,
            false,
            Some(Duration::from_secs(7200)),
        );

        let bucket_name = format!("qovery-test-bucket-{}", generate_id());
        let object_key = format!("test-object-{}", generate_id());

        aws_os
            .create_bucket(bucket_name.as_str())
            .expect("error while creating object-storage bucket");

        let temp_file = NamedTempFile::new().expect("error while creating tempfile");

        // compute:
        let result = aws_os.put(
            bucket_name.as_str(),
            object_key.as_str(),
            temp_file.into_temp_path().to_str().unwrap(),
        );

        // validate:
        assert!(result.is_ok());
        assert!(aws_os.get(bucket_name.as_str(), object_key.as_str(), false).is_ok());

        // clean-up:
        assert!(aws_os.delete_bucket(bucket_name.as_str()).is_ok());

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
        let context = context_for_resource(Uuid::new_v4(), Uuid::new_v4());
        let secrets = FuncTestsSecrets::new();
        let id = generate_id();
        let name = format!("test-{id}");
        let aws_access_key = secrets.AWS_ACCESS_KEY_ID.expect("AWS_ACCESS_KEY_ID is not set");
        let aws_secret_key = secrets.AWS_SECRET_ACCESS_KEY.expect("AWS_SECRET_ACCESS_KEY is not set");
        let aws_region_raw = secrets.AWS_DEFAULT_REGION.expect("AWS_DEFAULT_REGION is not set");
        let aws_region = AwsRegion::from_str(aws_region_raw.as_str())
            .unwrap_or_else(|_| panic!("AWS region `{aws_region_raw}` seems not to be valid"));

        let aws_os = S3::new(
            context,
            id.to_string(),
            name,
            aws_access_key,
            aws_secret_key,
            aws_region,
            false,
            Some(Duration::from_secs(7200)),
        );

        let bucket_name = format!("qovery-test-bucket-{}", generate_id());
        let object_key = format!("test-object-{}", generate_id());

        aws_os
            .create_bucket(bucket_name.as_str())
            .expect("error while creating object-storage bucket");

        let temp_file = NamedTempFile::new().expect("error while creating tempfile");
        let tempfile_path = temp_file.into_temp_path();
        let tempfile_path = tempfile_path.to_str().unwrap();

        aws_os
            .put(bucket_name.as_str(), object_key.as_str(), tempfile_path)
            .unwrap_or_else(|_| panic!("error while putting file {tempfile_path} into bucket {bucket_name}"));

        // compute:
        let result = aws_os.get(bucket_name.as_str(), object_key.as_str(), false);

        // validate:
        assert!(result.is_ok());

        // clean-up:
        assert!(aws_os.delete_bucket(bucket_name.as_str()).is_ok());

        test_name.to_string()
    })
}

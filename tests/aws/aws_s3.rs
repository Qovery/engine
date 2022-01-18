use qovery_engine::cloud_provider::aws::regions::AwsRegion;
use qovery_engine::object_storage::s3::S3;
use qovery_engine::object_storage::ObjectStorage;
use std::str::FromStr;
use tempfile::NamedTempFile;
use test_utilities::aws::AWS_RESOURCE_TTL_IN_SECONDS;
use test_utilities::utilities::{context, generate_id, FuncTestsSecrets};

#[cfg(feature = "test-aws-infra")]
#[test]
fn test_delete_bucket() {
    // setup:
    let context = context("fake_orga_id", "fake_cluster_id");
    let secrets = FuncTestsSecrets::new();
    let id = generate_id();
    let name = format!("test-{}", id.to_string());
    let aws_access_key = secrets.AWS_ACCESS_KEY_ID.expect("AWS_ACCESS_KEY_ID is not set");
    let aws_secret_key = secrets.AWS_SECRET_ACCESS_KEY.expect("AWS_SECRET_ACCESS_KEY is not set");
    let aws_region_raw = secrets.AWS_DEFAULT_REGION.expect("AWS_DEFAULT_REGION is not set");
    let aws_region = AwsRegion::from_str(aws_region_raw.as_str())
        .expect(format!("AWS region `{}` seems not to be valid", aws_region_raw).as_str());

    let aws_os = S3::new(
        context,
        id.to_string(),
        name.to_string(),
        aws_access_key,
        aws_secret_key,
        aws_region.clone(),
        false,
        Some(AWS_RESOURCE_TTL_IN_SECONDS),
    );

    let bucket_name = format!("qovery-test-bucket-{}", generate_id());

    aws_os
        .create_bucket(bucket_name.as_str())
        .expect(format!("error while creating S3 bucket in `{}`", aws_region.to_aws_format()).as_str());

    // compute:
    let result = aws_os.delete_bucket(bucket_name.as_str());

    // validate:
    assert!(
        result.is_ok(),
        "Delete bucket failed in `{}`",
        aws_region.to_aws_format()
    );
    assert!(
        !aws_os.bucket_exists(bucket_name.as_str()),
        "Delete bucket failed in `{}`, bucket still exists",
        aws_region.to_aws_format()
    );
}

#[cfg(feature = "test-aws-infra")]
#[test]
fn test_create_bucket() {
    // setup:
    let context = context("fake_orga_id", "fake_cluster_id");
    let secrets = FuncTestsSecrets::new();
    let id = generate_id();
    let name = format!("test-{}", id.to_string());
    let aws_access_key = secrets.AWS_ACCESS_KEY_ID.expect("AWS_ACCESS_KEY_ID is not set");
    let aws_secret_key = secrets.AWS_SECRET_ACCESS_KEY.expect("AWS_SECRET_ACCESS_KEY is not set");
    let aws_region_raw = secrets.AWS_DEFAULT_REGION.expect("AWS_DEFAULT_REGION is not set");
    let aws_region = AwsRegion::from_str(aws_region_raw.as_str())
        .expect(format!("AWS region `{}` seems not to be valid", aws_region_raw).as_str());

    let aws_os = S3::new(
        context,
        id.to_string(),
        name.to_string(),
        aws_access_key,
        aws_secret_key,
        aws_region.clone(),
        false,
        Some(AWS_RESOURCE_TTL_IN_SECONDS),
    );

    let bucket_name = format!("qovery-test-bucket-{}", generate_id());

    // compute:
    let result = aws_os.create_bucket(bucket_name.as_str());

    // validate:
    assert!(
        result.is_ok(),
        "Create bucket failed in `{}`",
        aws_region.to_aws_format()
    );
    assert!(
        aws_os.bucket_exists(bucket_name.as_str()),
        "Create bucket failed in `{}`, bucket doesn't exist",
        aws_region.to_aws_format()
    );

    // clean-up:
    aws_os.delete_bucket(bucket_name.as_str()).unwrap_or_else(|_| {
        panic!(
            "error deleting S3 bucket `{}` in `{}`",
            bucket_name,
            aws_region.to_aws_format()
        )
    });
}

#[cfg(feature = "test-aws-infra")]
#[test]
fn test_recreate_bucket() {
    // setup:
    let context = context("fake_orga_id", "fake_cluster_id");
    let secrets = FuncTestsSecrets::new();
    let id = generate_id();
    let name = format!("test-{}", id.to_string());
    let aws_access_key = secrets.AWS_ACCESS_KEY_ID.expect("AWS_ACCESS_KEY_ID is not set");
    let aws_secret_key = secrets.AWS_SECRET_ACCESS_KEY.expect("AWS_SECRET_ACCESS_KEY is not set");
    let aws_region_raw = secrets.AWS_DEFAULT_REGION.expect("AWS_DEFAULT_REGION is not set");
    let aws_region = AwsRegion::from_str(aws_region_raw.as_str())
        .expect(format!("AWS region `{}` seems not to be valid", aws_region_raw).as_str());

    let aws_os = S3::new(
        context,
        id.to_string(),
        name.to_string(),
        aws_access_key,
        aws_secret_key,
        aws_region.clone(),
        false,
        Some(AWS_RESOURCE_TTL_IN_SECONDS),
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
    aws_os
        .delete_bucket(bucket_name.as_str())
        .unwrap_or_else(|_| panic!("error deleting S3 bucket {}", bucket_name));
}

#[cfg(feature = "test-aws-infra")]
#[test]
fn test_put_file() {
    // setup:
    let context = context("fake_orga_id", "fake_cluster_id");
    let secrets = FuncTestsSecrets::new();
    let id = generate_id();
    let name = format!("test-{}", id.to_string());
    let aws_access_key = secrets.AWS_ACCESS_KEY_ID.expect("AWS_ACCESS_KEY_ID is not set");
    let aws_secret_key = secrets.AWS_SECRET_ACCESS_KEY.expect("AWS_SECRET_ACCESS_KEY is not set");
    let aws_region_raw = secrets.AWS_DEFAULT_REGION.expect("AWS_DEFAULT_REGION is not set");
    let aws_region = AwsRegion::from_str(aws_region_raw.as_str())
        .expect(format!("AWS region `{}` seems not to be valid", aws_region_raw).as_str());

    let aws_os = S3::new(
        context,
        id.to_string(),
        name.to_string(),
        aws_access_key,
        aws_secret_key,
        aws_region.clone(),
        false,
        Some(AWS_RESOURCE_TTL_IN_SECONDS),
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
    aws_os
        .delete_bucket(bucket_name.as_str())
        .unwrap_or_else(|_| panic!("error deleting S3 bucket {}", bucket_name));
}

#[cfg(feature = "test-aws-infra")]
#[test]
fn test_get_file() {
    // setup:
    let context = context("fake_orga_id", "fake_cluster_id");
    let secrets = FuncTestsSecrets::new();
    let id = generate_id();
    let name = format!("test-{}", id.to_string());
    let aws_access_key = secrets.AWS_ACCESS_KEY_ID.expect("AWS_ACCESS_KEY_ID is not set");
    let aws_secret_key = secrets.AWS_SECRET_ACCESS_KEY.expect("AWS_SECRET_ACCESS_KEY is not set");
    let aws_region_raw = secrets.AWS_DEFAULT_REGION.expect("AWS_DEFAULT_REGION is not set");
    let aws_region = AwsRegion::from_str(aws_region_raw.as_str())
        .expect(format!("AWS region `{}` seems not to be valid", aws_region_raw).as_str());

    let aws_os = S3::new(
        context,
        id.to_string(),
        name.to_string(),
        aws_access_key,
        aws_secret_key,
        aws_region.clone(),
        false,
        Some(AWS_RESOURCE_TTL_IN_SECONDS),
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
        .unwrap_or_else(|_| panic!("error while putting file {} into bucket {}", tempfile_path, bucket_name));

    // compute:
    let result = aws_os.get(bucket_name.as_str(), object_key.as_str(), false);

    // validate:
    assert!(result.is_ok());

    // clean-up:
    aws_os
        .delete_bucket(bucket_name.as_str())
        .unwrap_or_else(|_| panic!("error deleting S3 bucket {}", bucket_name));
}

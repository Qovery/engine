use std::str::FromStr;

use rusoto_core::credential::StaticProvider;
use rusoto_core::{Client, Region};
use rusoto_s3::{
    CreateBucketConfiguration, CreateBucketError, CreateBucketRequest, GetObjectError,
    GetObjectRequest, ListObjectsV2Output, ListObjectsV2Request, PutBucketVersioningRequest,
    PutObjectRequest, S3Client, VersioningConfiguration, S3,
};

use qovery_engine::s3;
use qovery_engine::s3::{delete_bucket, get_default_region_for_us};
use test_utilities::aws::{aws_access_key_id, aws_secret_access_key, AWS_REGION_FOR_S3};
use test_utilities::utilities::init;

#[test]
fn delete_s3_bucket() {
    init();
    let bucket_name = "my-test-bucket";
    let credentials = StaticProvider::new(
        aws_access_key_id().to_string(),
        aws_secret_access_key().to_string(),
        None,
        None,
    );

    let creation = s3::create_bucket(
        aws_access_key_id().as_str(),
        aws_secret_access_key().as_str(),
        bucket_name,
    );
    match creation {
        Ok(out) => println!("Yippee Ki Yay"),
        Err(e) => println!("While creating the bucket {}", e),
    }

    let delete = delete_bucket(
        aws_access_key_id().as_str(),
        aws_secret_access_key().as_str(),
        bucket_name,
    );
    match delete {
        Ok(out) => println!("Yippee Ki Yay"),
        Err(e) => println!("While deleting the bucket {}", e),
    }
}

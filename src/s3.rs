use crate::runtime::async_run;
use rusoto_core::{Client, HttpClient, Region, RusotoError};
use rusoto_credential::StaticProvider;
use rusoto_s3::{
    CreateBucketConfiguration, CreateBucketError, CreateBucketRequest, PutBucketVersioningError,
    PutBucketVersioningRequest, S3Client, VersioningConfiguration, S3,
};
use std::io::{Error, ErrorKind};

pub fn create_bucket(
    access_key_id: String,
    secret_access_key: String,
    region: Region,
    bucket_name: String,
) -> Result<(), Error> {
    let credentials = StaticProvider::new(access_key_id, secret_access_key, None, None);
    let client = Client::new_with(credentials, HttpClient::new().unwrap());
    let s3_client = S3Client::new_with_client(client, region.clone());

    let mut bc = CreateBucketRequest::default();
    bc.acl = Some("private".to_string());
    bc.bucket = bucket_name;
    bc.create_bucket_configuration = Some(CreateBucketConfiguration {
        location_constraint: Some(region.name().to_string()),
    });

    let create_bucket_output = s3_client.create_bucket(bc.clone());
    let r = async_run(create_bucket_output);

    match r {
        Err(err) => match err {
            RusotoError::Service(s) => match s {
                CreateBucketError::BucketAlreadyExists(x) => info!("bucket already exists"),
                CreateBucketError::BucketAlreadyOwnedByYou(x) => {}
            },
            RusotoError::Unknown(r) => error!("{}", r.body_as_str()),
            _ => {
                return Err(Error::new(
                    ErrorKind::Other,
                    "something goes wrong while creating the S3 bucket",
                ))
            }
        },
        _ => {}
    };

    let bucket_versioning_output = s3_client.put_bucket_versioning(PutBucketVersioningRequest {
        bucket: bc.bucket.clone(),
        content_md5: None,
        mfa: None,
        versioning_configuration: VersioningConfiguration {
            mfa_delete: None,
            //https://docs.aws.amazon.com/AmazonS3/latest/API/API_PutBucketVersioning.html
            status: Some("Enabled".to_string()),
        },
    });

    let r = async_run(bucket_versioning_output);

    match r {
        Err(err) => match err {
            RusotoError::Unknown(r) => {
                error!("{}", r.body_as_str());
                Err(Error::new(ErrorKind::Other, r.body_as_str()))
            }
            _ => {
                return Err(Error::new(
                    ErrorKind::Other,
                    "something goes wrong while versioning the S3 bucket",
                ))
            }
        },
        Ok(x) => Ok(()),
    }
}

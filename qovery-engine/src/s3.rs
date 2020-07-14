use crate::runtime::async_run;
use rusoto_core::{Client, HttpClient, Region, RusotoError};
use rusoto_credential::StaticProvider;
use rusoto_s3::{
    CreateBucketConfiguration, CreateBucketError, CreateBucketRequest, GetObjectError,
    GetObjectRequest, PutBucketVersioningRequest, S3Client, VersioningConfiguration, S3,
};
use std::fs;
use std::fs::File;
use std::io::{Error, ErrorKind, Read, Write};
use std::path::Path;

pub fn create_bucket(
    access_key_id: &str,
    secret_access_key: &str,
    region: &Region,
    bucket_name: &str,
) -> Result<(), Error> {
    let access_key_id = access_key_id.to_string();
    let secret_access_key = secret_access_key.to_string();
    let bucket_name = bucket_name.to_string();

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

    // FIXME: return a custom S3Error?
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

pub type FileContent = String;

pub fn get_object(
    access_key_id: &str,
    secret_access_key: &str,
    region: &Region,
    bucket_name: &str,
    object_key: &str,
) -> Result<FileContent, Error> {
    let credentials = StaticProvider::new(
        access_key_id.to_string(),
        secret_access_key.to_string(),
        None,
        None,
    );
    let client = Client::new_with(credentials, HttpClient::new().unwrap());
    let s3_client = S3Client::new_with_client(client, region.clone());

    let mut or = GetObjectRequest::default();
    or.bucket = bucket_name.to_string();
    or.key = object_key.to_string();

    let get_object_output = s3_client.get_object(or);
    let r = async_run(get_object_output);

    match r {
        Ok(x) => {
            let mut s = String::new();
            x.body.unwrap().into_blocking_read().read_to_string(&mut s);
            Ok(s)
        }
        Err(err) => match err {
            RusotoError::Service(s) => match s {
                GetObjectError::NoSuchKey(x) => {
                    info!("no such key: {}", x.as_str());
                    return Err(Error::new(
                        ErrorKind::NotFound,
                        format!("no such key: {}", x.as_str()),
                    ));
                }
            },
            RusotoError::Unknown(r) => {
                error!("{}", r.body_as_str());
                return Err(Error::new(
                    ErrorKind::Other,
                    format!(
                        "something goes wrong while getting object {} in the S3 bucket {}",
                        object_key, bucket_name
                    ),
                ));
            }
            _ => {
                return Err(Error::new(
                    ErrorKind::Other,
                    format!(
                        "something goes wrong while getting object {} in the S3 bucket {}",
                        object_key, bucket_name
                    ),
                ))
            }
        },
    }
}

pub fn get_kubernetes_config_file<P>(
    access_key_id: &str,
    secret_access_key: &str,
    region: &Region,
    kubernetes_config_bucket_name: &str,
    kubernetes_config_object_key: &str,
    file_path: P,
) -> Result<File, Error>
where
    P: AsRef<Path>,
{
    let file_content = crate::s3::get_object(
        access_key_id,
        secret_access_key,
        region,
        kubernetes_config_bucket_name,
        kubernetes_config_object_key,
    )?;

    let mut kubernetes_config_file = File::create(file_path.as_ref())?;
    let _ = kubernetes_config_file.write(file_content.as_bytes())?;

    Ok(kubernetes_config_file)
}

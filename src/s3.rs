use std::fmt::Display;
use std::fs::{read_to_string, File};
use std::io::{Error, ErrorKind, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::str::FromStr;
use std::{fs, io};

use retry::delay::Fibonacci;
use retry::OperationResult;
use rusoto_core::{Client, HttpClient, Region, RusotoError};
use rusoto_credential::StaticProvider;
use rusoto_s3::{
    CreateBucketConfiguration, CreateBucketError, CreateBucketRequest, GetObjectError,
    GetObjectRequest, ListObjectsV2Output, ListObjectsV2Request, PutBucketVersioningRequest,
    S3Client, VersioningConfiguration, S3,
};

use crate::cmd::utilities::exec_with_envs;
use crate::error::{SimpleError, SimpleErrorKind};
use crate::runtime::async_run;

pub const AWS_REGION_FOR_S3_US: &str = "ap-south-1";

pub type FileContent = String;

pub fn create_bucket(
    access_key_id: &str,
    secret_access_key: &str,
    bucket_name: &str,
) -> Result<(), SimpleError> {
    exec_with_envs(
        "aws",
        vec!["s3api", "create-bucket", "--bucket", &bucket_name],
        vec![
            ("AWS_ACCESS_KEY_ID", &access_key_id),
            ("AWS_SECRET_ACCESS_KEY", &secret_access_key),
        ],
    )
}

pub fn get_object(
    access_key_id: &str,
    secret_access_key: &str,
    region: &Region,
    bucket_name: &str,
    object_key: &str,
) -> Result<FileContent, SimpleError> {
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

    let _err = SimpleError::new(
        SimpleErrorKind::Other,
        Some(format!(
            "something goes wrong while getting object {} in the S3 bucket {}",
            object_key, bucket_name
        )),
    );

    match r {
        Ok(x) => {
            let mut s = String::new();
            x.body.unwrap().into_blocking_read().read_to_string(&mut s);

            if s.is_empty() {
                // this handle a case where the request succeeds but contains an empty body.
                // https://github.com/rusoto/rusoto/issues/1822
                let r_from_aws_cli = get_object_via_aws_cli(
                    access_key_id,
                    secret_access_key,
                    bucket_name,
                    object_key,
                )?;
                return Ok(r_from_aws_cli);
            }
            Ok(s)
        }
        Err(err) => {
            return match err {
                RusotoError::Service(s) => match s {
                    GetObjectError::NoSuchKey(x) => {
                        info!("no such key '{}': {}", object_key, x.as_str());
                        Err(SimpleError::new(
                            SimpleErrorKind::Other,
                            Some(format!("no such key '{}': {}", object_key, x.as_str())),
                        ))
                    }
                },
                RusotoError::Unknown(r) => {
                    let r_from_aws_cli = get_object_via_aws_cli(
                        access_key_id,
                        secret_access_key,
                        bucket_name,
                        object_key,
                    );

                    match r_from_aws_cli {
                        Ok(..) => Ok(r_from_aws_cli.unwrap()),
                        Err(err) => {
                            if let Some(message) = err.message {
                                error!("{}", message);
                            }

                            Err(_err)
                        }
                    }
                }
                _ => Err(_err),
            };
        }
    }
}

/// gets an aws s3 object using aws-cli
/// used as a failover when rusoto_s3 acts up
fn get_object_via_aws_cli(
    access_key_id: &str,
    secret_access_key: &str,
    bucket_name: &str,
    object_key: &str,
) -> Result<FileContent, SimpleError> {
    let s3_url = format!("s3://{}/{}", bucket_name, object_key);
    let local_path = format!("/tmp/{}", object_key);

    exec_with_envs(
        "aws",
        vec!["s3", "cp", &s3_url, &local_path],
        vec![
            ("AWS_ACCESS_KEY_ID", &access_key_id),
            ("AWS_SECRET_ACCESS_KEY", &secret_access_key),
        ],
    )?;

    let s = read_to_string(&local_path)?;
    Ok(s)
}

pub fn get_kubernetes_config_file<P>(
    access_key_id: &str,
    secret_access_key: &str,
    region: &Region,
    kubernetes_config_bucket_name: &str,
    kubernetes_config_object_key: &str,
    file_path: P,
) -> Result<File, SimpleError>
where
    P: AsRef<Path>,
{
    // return the file if it already exists
    let _ = match File::open(file_path.as_ref()) {
        Ok(f) => return Ok(f),
        Err(_) => {}
    };

    let file_content_result = retry::retry(Fibonacci::from_millis(3000).take(5), || {
        let file_content = crate::s3::get_object_via_aws_cli(
            access_key_id,
            secret_access_key,
            kubernetes_config_bucket_name,
            kubernetes_config_object_key,
        );
        match file_content {
            Ok(file_content) => OperationResult::Ok(file_content),
            Err(err) => {
                warn!(
                    "Can't download the kubernetes config file {} stored on {}, please check access key and secrets",
                    kubernetes_config_object_key, kubernetes_config_bucket_name
                );
                OperationResult::Retry(err)
            }
        }
    });

    let file_content = match file_content_result {
        Ok(file_content) => file_content,
        Err(_) => {
            return Err(SimpleError::new(
                SimpleErrorKind::Other,
                Some("file content is empty (retry failed multiple times) - which is not the expected content - what's wrong?"),
            ));
        }
    };

    let mut kubernetes_config_file = File::create(file_path.as_ref())?;
    let _ = kubernetes_config_file.write(file_content.as_bytes())?;
    // removes warning kubeconfig is (world/group) readable
    let metadata = kubernetes_config_file.metadata()?;
    let mut permissions = metadata.permissions();
    permissions.set_mode(0o400);
    fs::set_permissions(file_path.as_ref(), permissions)?;
    Ok(kubernetes_config_file)
}

pub fn list_objects_in(
    access_key_id: &str,
    secret_access_key: &str,
    bucket_name: &str,
) -> Result<ListObjectsV2Output, SimpleError> {
    let credentials = StaticProvider::new(
        access_key_id.to_string(),
        secret_access_key.to_string(),
        None,
        None,
    );
    let client = Client::new_with(credentials, HttpClient::new().unwrap());
    let s3_client = S3Client::new_with_client(client, get_default_region_for_us());

    let mut list_request = ListObjectsV2Request::default();
    list_request.bucket = bucket_name.to_string();

    let lis_object = s3_client.list_objects_v2(list_request);
    let objects_in = async_run(lis_object);

    match objects_in {
        Ok(objects) => Ok(objects),
        Err(err) => Err(SimpleError::new(
            SimpleErrorKind::Other,
            Some(format!("error listing objects from s3 {:?}", err)),
        )),
    }
}

// delete bucket implement by default objects deletion
pub fn delete_bucket(
    access_key_id: &str,
    secret_access_key: &str,
    bucket_name: &str,
) -> Result<(), SimpleError> {
    info!("Deleting S3 Bucket {}", bucket_name.clone());
    match exec_with_envs(
        "aws",
        vec![
            "s3",
            "rb",
            "--force",
            "--bucket",
            format!("s3://{}", bucket_name).as_str(),
        ],
        vec![
            ("AWS_ACCESS_KEY_ID", &access_key_id),
            ("AWS_SECRET_ACCESS_KEY", &secret_access_key),
        ],
    ) {
        Ok(o) => {
            info!("Successfuly delete bucket");
            return Ok(o);
        }
        Err(e) => {
            error!(
                "while deleting bucket {}",
                e.message.as_ref().unwrap_or(&"".into())
            );
            return Err(e);
        }
    }
}

pub fn get_default_region_for_us() -> Region {
    Region::from_str(AWS_REGION_FOR_S3_US).unwrap()
}

pub fn push_object(
    access_key_id: &str,
    secret_access_key: &str,
    bucket_name: &str,
    object_key: &str,
    local_file_path: &str,
) -> Result<(), SimpleError> {
    info!(
        "Pushing object {} to bucket {}",
        local_file_path.clone(),
        bucket_name.clone()
    );
    match exec_with_envs(
        "aws",
        vec![
            "s3",
            "cp",
            local_file_path,
            format!("s3://{}/{}", bucket_name, object_key).as_str(),
        ],
        vec![
            ("AWS_ACCESS_KEY_ID", &access_key_id),
            ("AWS_SECRET_ACCESS_KEY", &secret_access_key),
        ],
    ) {
        Ok(o) => {
            info!("Successfully uploading the object on bucket");
            return Ok(o);
        }
        Err(e) => {
            error!(
                "While uploading object {}",
                e.message.as_ref().unwrap_or(&"".into())
            );
            return Err(e);
        }
    }
}

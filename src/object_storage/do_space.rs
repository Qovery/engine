use crate::error::{SimpleError, SimpleErrorKind};
use crate::s3::get_object;
use rusoto_core::{Client, HttpClient, Region, RusotoError};
use rusoto_credential::StaticProvider;
use rusoto_s3::{GetObjectError, GetObjectOutput, GetObjectRequest, S3Client, S3};
use std::io::Read;
use std::io::{Cursor, Error};
use tokio::runtime::{Builder, Runtime};
struct Sync_do_space {
    client: S3Client,
    runtime: Runtime,
}

// implement synchronous way to download s3 objects... yeah !
impl Sync_do_space {
    fn new(access_key_id: &str, secret_access_key: &str, region: &str) -> Result<Self, Error> {
        let credentials = StaticProvider::new(
            access_key_id.to_string(),
            secret_access_key.to_string(),
            None,
            None,
        );
        let client = Client::new_with(credentials, HttpClient::new().unwrap());
        let endpoint_region = Region::Custom {
            name: region.to_string(),
            endpoint: format!("https://{}.digitaloceanspaces.com",region),
        };
        Ok(Sync_do_space {
            client: S3Client::new_with_client(client, endpoint_region),
            runtime: Builder::new().basic_scheduler().enable_all().build()?,
        })
    }

    fn get_object(&mut self, request: GetObjectRequest) -> Result<String, SimpleError> {
        let response = self.runtime.block_on(self.client.get_object(request));
        match response {
            Ok(res) => {
                let mut body = String::new();
                res.body
                    .unwrap()
                    .into_blocking_read()
                    .read_to_string(&mut body);
                Ok(body)
            }
            Err(e) => Err(SimpleError::new(
                SimpleErrorKind::Other,
                Some(e.to_string()),
            )),
        }
    }
}

pub fn download_space_object(
    access_key_id: &str,
    secret_access_key: &str,
    bucket_name: &str,
    object_key: &str,
    region: &str
) -> Result<String, SimpleError> {
    let sync_do_space = Sync_do_space::new(access_key_id, secret_access_key,region);
    match sync_do_space {
        Ok(mut client) => {
            let mut or = GetObjectRequest::default();
            or.bucket = bucket_name.to_string();
            or.key = object_key.to_string();
            let res_body = client.get_object(or);
            match res_body {
                Ok(body) => Ok(body),
                Err(e) => Err(e),
            }
        }
        Err(e) => Err(SimpleError::new(
            SimpleErrorKind::Other,
            Some(e.to_string()),
        )),
    }
}

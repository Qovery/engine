use crate::s3::get_object;
use rusoto_core::{Client, HttpClient, Region};
use rusoto_credential::StaticProvider;
use rusoto_s3::{GetObjectRequest, S3Client, S3};
use tokio::{fs::File, io};

pub(crate) async fn download_space_object(
    access_key_id: &str,
    secret_access_key: &str,
    bucket_name: &str,
    object_key: &str,
    region: &str,
    path_to_download: &str,
) {
    //Digital ocean doesn't implement any space download, it use the generic AWS SDK
    let region = Region::Custom {
        name: region.to_string(),
        endpoint: format!("https://{}.digitaloceanspaces.com", region),
    };
    let credentials = StaticProvider::new(
        access_key_id.to_string(),
        secret_access_key.to_string(),
        None,
        None,
    );

    let client = Client::new_with(credentials, HttpClient::new().unwrap());
    let s3_client = S3Client::new_with_client(client, region.clone());
    let object = s3_client
        .get_object(GetObjectRequest {
            bucket: bucket_name.to_string(),
            key: object_key.to_string(),
            ..Default::default()
        })
        .await;

    match object {
        Ok(mut obj_bod) => {
            let body = obj_bod.body.take();
            let mut body = body.unwrap().into_async_read();
            let file = File::create(path_to_download.clone()).await;
            match file {
                Ok(mut created_file) => match io::copy(&mut body, &mut created_file).await {
                    Ok(_) => info!("File {} is well downloaded", path_to_download),
                    Err(e) => error!("{:?}", e),
                },
                Err(e) => error!("Unable to create file, maybe this file already exists..."),
            }
        }
        Err(e) => error!("{:?}", e),
    };
}

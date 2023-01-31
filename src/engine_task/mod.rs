use crate::cloud_provider::aws::regions::AwsRegion;

use crate::io_models::context::Context;
use crate::io_models::engine_request::Archive;
use crate::object_storage::errors::ObjectStorageError;
use crate::object_storage::ObjectStorage;
use chrono::{DateTime, Utc};
use std::borrow::Cow;

pub mod core_service_api;
pub mod environment_task;
pub mod infrastructure_task;

pub trait Task: Send + Sync {
    fn created_at(&self) -> &DateTime<Utc>;
    fn id(&self) -> &str;
    fn run(&self);
    fn cancel(&self) -> bool;
    fn cancel_checker(&self) -> Box<dyn Fn() -> bool>;
}

fn basename(path: &str, sep: char) -> Cow<str> {
    let pieces = path.split(sep);
    match pieces.last() {
        Some(p) => p.into(),
        None => path.into(),
    }
}

fn upload_s3_file(
    context: &Context,
    archive: Option<&Archive>,
    file_path: &str,
    region: AwsRegion,
    bucket_ttl: i32,
) -> Result<(), ObjectStorageError> {
    let archive = match archive {
        Some(archive) => archive,
        None => {
            info!("no archive upload (request.archive is None)");
            return Ok(());
        }
    };

    let object_key = format!("{}/{}", context.organization_short_id(), basename(file_path, '/'));

    info!(
        "Sending file {} to bucket {} object {} with access_key_id '{}' and secret_access_key '{}'",
        file_path,
        archive.bucket_name.as_str(),
        object_key.as_str(),
        archive.access_key_id.as_str(),
        archive.secret_access_key.as_str(),
    );

    // I am using this s3 object directly to avoid reinventing the wheel.
    let ttl = match bucket_ttl {
        0 => None,
        _ => Some(bucket_ttl),
    };
    let s3 = crate::object_storage::s3::S3::new(
        context.clone(),
        "archive-123abc".to_string(),
        "archive-s3".to_string(),
        archive.access_key_id.to_string(),
        archive.secret_access_key.to_string(),
        region,
        true,
        ttl,
    );

    match s3.put(archive.bucket_name.as_str(), object_key.as_str(), file_path) {
        Ok(_) => {
            info!("Archive successfully pushed to Qovery S3");
            Ok(())
        }
        Err(err) => {
            warn!("Error while pushing archive to s3, {}", err);
            Err(err)
        }
    }
}

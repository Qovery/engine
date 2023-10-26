use crate::models::gcp::{Credentials, CredentialsError};
use crate::models::ToCloudProviderFormat;
use crate::object_storage::Bucket;
use crate::runtime::block_on;
use crate::services::gcp::object_storage_regions::GcpStorageRegion;
use chrono::Duration;
use google_cloud_auth::credentials::CredentialsFile;
use google_cloud_storage::http::buckets::lifecycle::rule::ActionType;
use google_cloud_storage::http::buckets::Bucket as GcpBucket;
use std::str::FromStr;

/// Handle conversion and deal with external types for Google cloud
/// defined here https://github.com/yoshidan/google-cloud-rust
/// Keeping it isolated prevent from high coupling with third party crate

pub fn new_gcp_credentials_file_from_credentials(
    credentials: Credentials,
) -> Result<CredentialsFile, CredentialsError> {
    block_on(CredentialsFile::new_from_str(credentials.to_cloud_provider_format().as_str())).map_err(|e| {
        CredentialsError::CannotCreateCredentials {
            raw_error_message: e.to_string(),
        }
    })
}

impl TryFrom<GcpBucket> for Bucket<GcpStorageRegion> {
    type Error = String;

    fn try_from(value: GcpBucket) -> Result<Self, Self::Error> {
        let gcp_storage_region = match GcpStorageRegion::from_str(value.location.as_str()) {
            Ok(r) => r,
            Err(e) => return Err(e),
        };

        Ok(Bucket {
            name: value.name,
            ttl: match &value.lifecycle {
                Some(lifecycle) => lifecycle
                    .rule
                    .iter()
                    .find(|r| match &r.action {
                        Some(action) => action.r#type == ActionType::Delete,
                        _ => false,
                    })
                    .and_then(|r| r.condition.clone())
                    .map(|c| Duration::days(i64::from(c.age))),
                None => None,
            },
            location: gcp_storage_region,
            labels: value.labels,
        })
    }
}

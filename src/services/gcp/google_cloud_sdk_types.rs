use crate::cloud_provider::gcp::locations::GcpRegion;
use crate::container_registry::{DockerImage, Repository};
use crate::models::gcp::io::JsonCredentials as JsonCredentialsIo;
use crate::models::gcp::{CredentialsError, JsonCredentials};
use crate::models::ToCloudProviderFormat;
use crate::object_storage::{Bucket, BucketRegion};
use crate::runtime::block_on;
use crate::services::gcp::object_storage_regions::GcpStorageRegion;
use google_cloud_auth::credentials::CredentialsFile;
use google_cloud_googleapis::devtools::artifact_registry::v1::{
    DockerImage as GcpDockerImage, Package as GcpPackage, Repository as GcpRepository,
};
use google_cloud_storage::http::buckets::lifecycle::rule::ActionType;
use google_cloud_storage::http::buckets::Bucket as GcpBucket;
use regex::Regex;
use std::str::FromStr;
use std::time::Duration;

/// Handle conversion and deal with external types for Google cloud
/// defined here https://github.com/yoshidan/google-cloud-rust
/// Keeping it isolated prevent from high coupling with third party crate

pub fn new_gcp_credentials_file_from_credentials(
    credentials: JsonCredentials,
) -> Result<CredentialsFile, CredentialsError> {
    let credentials_json_str = serde_json::to_string(&JsonCredentialsIo::from(credentials)).map_err(|e| {
        CredentialsError::CannotCreateCredentials {
            raw_error_message: e.to_string(),
        }
    })?;

    block_on(CredentialsFile::new_from_str(&credentials_json_str)).map_err(|e| {
        CredentialsError::CannotCreateCredentials {
            raw_error_message: e.to_string(),
        }
    })
}

impl TryFrom<GcpBucket> for Bucket {
    type Error = String;

    fn try_from(gcp_bucket: GcpBucket) -> Result<Self, Self::Error> {
        let gcp_storage_region = match GcpStorageRegion::from_str(gcp_bucket.location.as_str()) {
            Ok(r) => r,
            Err(e) => return Err(e),
        };

        Ok(Bucket {
            name: gcp_bucket.name,
            ttl: match &gcp_bucket.lifecycle {
                Some(lifecycle) => lifecycle
                    .rule
                    .iter()
                    .find(|r| match &r.action {
                        Some(action) => action.r#type == ActionType::Delete,
                        _ => false,
                    })
                    .and_then(|r| r.condition.clone())
                    .map(|c| Duration::from_secs(c.age as u64 * 60 * 60 * 24)),
                None => None,
            },
            versioning_activated: match gcp_bucket.versioning {
                None => false,
                Some(v) => v.enabled,
            },
            location: BucketRegion::GcpRegion(gcp_storage_region.clone()),
            labels: gcp_bucket.labels,
        })
    }
}

// TODO(benjaminch): stick a test
pub fn from_gcp_repository(
    project_id: &str,
    location: GcpRegion,
    gcp_repository: GcpRepository,
) -> Result<Repository, String> {
    // extract repository element from fully qualified name
    // name contains the fully qualified elements (e.q "projects/project_id/locations/europe-west9/repositories/repository-for-documentation")
    let mut repository_parent = "".to_string(); // e.q "projects/project_id/locations/europe-west9"
    let mut repository_name = "".to_string();
    if let Ok(repository_re) = Regex::new(r"(?P<repository_parent>.*)/repositories/(?P<repository_name>.*)") {
        if let Some(cap) = repository_re.captures(gcp_repository.name.as_str()) {
            match (
                cap.name("repository_parent").map(|e| e.as_str()),
                cap.name("repository_name").map(|e| e.as_str()),
            ) {
                (Some(parent), Some(name)) => {
                    repository_parent = parent.to_string();
                    repository_name = name.to_string();
                }
                _ => {
                    return Err(format!(
                        "Cannot extract repository name and parent from fully qualified name: `{}`",
                        gcp_repository.name.as_str()
                    ))
                }
            }
        }
    }

    Ok(Repository {
        registry_id: repository_parent.to_string(),
        id: gcp_repository.name.to_string(), // fully qualified repository id
        name: repository_name.to_string(),
        uri: Some(format!(
            "{}-docker.pkg.dev/{}/{}/",
            location.to_cloud_provider_format(),
            project_id,
            repository_name
        )),
        ttl: None, // TODO(benjaminch): TTL to be added
        labels: Some(gcp_repository.labels),
    })
}

impl TryFrom<GcpDockerImage> for DockerImage {
    type Error = String;

    fn try_from(gcp_docker_image: GcpDockerImage) -> Result<Self, Self::Error> {
        // extract docker image element from fully qualified name
        // name contains the fully qualified elements (e.q "projects/project_id/locations/europe-west9/repositories/repository-for-documentation/image-name@sha256:xxxxxxxxxx")
        let mut repository_identifier = "".to_string(); // e.q "projects/project_id/locations/europe-west9"
        let mut docker_image_name = "".to_string(); // e.q image-name
        if let Ok(repository_re) = Regex::new(
            r"(?P<repository_identifier>projects/.*/locations/.*/repositories/.*)/dockerImages/(?P<docker_image_name>.*)@sha256:.*",
        ) {
            if let Some(cap) = repository_re.captures(gcp_docker_image.name.as_str()) {
                match (
                    cap.name("repository_identifier").map(|e| e.as_str()),
                    cap.name("docker_image_name").map(|e| e.as_str()),
                ) {
                    (Some(repository), Some(image_name)) => {
                        repository_identifier = repository.to_string();
                        docker_image_name = image_name.to_string();
                    }
                    _ => {
                        return Err(format!(
                            "Cannot extract docker image name and repository from fully qualified name: `{}`",
                            gcp_docker_image.name.as_str()
                        ))
                    }
                }
            }
        }

        Ok(DockerImage {
            repository_id: repository_identifier,
            name: docker_image_name,
            tag: match gcp_docker_image.tags.first() {
                Some(t) => t.to_string(),
                None => "".to_string(),
            }, // TODO(benjaminch): improve this
        })
    }
}

impl TryFrom<GcpPackage> for DockerImage {
    type Error = String;

    fn try_from(gcp_package: GcpPackage) -> Result<Self, Self::Error> {
        // extract docker image element from fully qualified name
        // name contains the fully qualified elements (e.q "projects/project_id/locations/europe-west9/repositories/repository-for-documentation/image-name@sha256:xxxxxxxxxx")
        let mut repository_identifier = "".to_string(); // e.q "projects/project_id/locations/europe-west9"
        let mut docker_image_name = "".to_string(); // e.q image-name
        if let Ok(repository_re) = Regex::new(
            r"(?P<repository_identifier>projects/.*/locations/.*/repositories/.*)/dockerImages/(?P<docker_image_name>.*)@sha256:.*",
        ) {
            if let Some(cap) = repository_re.captures(gcp_package.name.as_str()) {
                match (
                    cap.name("repository_identifier").map(|e| e.as_str()),
                    cap.name("docker_image_name").map(|e| e.as_str()),
                ) {
                    (Some(repository), Some(image_name)) => {
                        repository_identifier = repository.to_string();
                        docker_image_name = image_name.to_string();
                    }
                    _ => {
                        return Err(format!(
                            "Cannot extract docker image name and repository from fully qualified name: `{}`",
                            gcp_package.name.as_str()
                        ))
                    }
                }
            }
        }

        Ok(DockerImage {
            repository_id: repository_identifier,
            name: docker_image_name,
            tag: "".to_string(), // TODO(benjaminch): improve this
        })
    }
}

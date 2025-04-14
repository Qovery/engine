pub mod abort;
mod annotations_group;
pub mod application;
pub mod aws;
pub mod azure;
pub mod container;
pub mod database;
pub(crate) mod database_utils;
pub mod domain;
pub mod environment;
pub mod gcp;
pub mod helm_chart;
pub mod job;
pub mod kubernetes;
mod labels_group;
pub mod probe;
pub mod registry_image_source;
pub mod router;
pub mod scaleway;
pub mod selfmanaged;
pub mod terraform_service;
pub mod third_parties;
pub mod types;
pub mod utils;

pub trait ToCloudProviderFormat {
    /// Returns cloud provider string representation.
    fn to_cloud_provider_format(&self) -> &str;
}

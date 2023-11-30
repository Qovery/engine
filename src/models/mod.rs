pub mod application;
pub mod aws;
pub mod aws_ec2;
pub mod container;
pub mod database;
pub(crate) mod database_utils;
pub mod domain;
pub mod gcp;
pub mod helm_chart;
pub mod job;
pub mod kubernetes;
pub mod probe;
pub mod registry_image_source;
pub mod router;
pub mod scaleway;
pub mod third_parties;
pub mod types;

pub trait ToCloudProviderFormat {
    /// Returns cloud provider string representation.
    fn to_cloud_provider_format(&self) -> &str;
}

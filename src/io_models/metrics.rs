use serde::{Deserialize, Serialize};

use crate::infrastructure::models::cloud_provider::aws::regions::AwsRegion;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum MetricsIoConfig {
    AwsS3(AwsS3PrometheusConfig),
    GcpCloudStorage(GcpCloudStoragePrometheusConfig),
    Custom,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GcpCloudStoragePrometheusConfig {}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AwsS3PrometheusConfig {
    pub bucket_name: String,
    pub region: AwsRegion,
    pub endpoint: String,
    pub aws_iam_prometheus_role_arn: String,
}

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all(serialize = "snake_case", deserialize = "snake_case"))]
pub struct MetricsParameters {
    pub config: MetricsConfiguration,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all(serialize = "snake_case", deserialize = "snake_case"))]
pub enum MetricsConfiguration {
    MetricsInstalledByQovery {
        // INFO (ENG-1986) ATM this field should be filled only for dedicated Qovery internal clusters
        install_prometheus_adapter: bool,
        enable_redundancy: Option<bool>,
    },
    AwsS3 {
        region: String,
        bucket_name: String,
        aws_iam_prometheus_role_arn: String,
        endpoint: String,
    },
}

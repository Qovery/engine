use std::fmt::Display;
use std::fmt::Formatter;

use crate::environment::models::types::CloudProvider;
use crate::environment::models::types::AWS;
use crate::environment::models::ToCloudProviderFormat;
use crate::infrastructure::models::cloud_provider::Kind;

mod database;
mod database_utils;
mod job;
mod router;

pub struct AwsAppExtraSettings {}
pub struct AwsDbExtraSettings {}
pub struct AwsRouterExtraSettings {}

impl CloudProvider for AWS {
    type AppExtraSettings = AwsAppExtraSettings;
    type DbExtraSettings = AwsDbExtraSettings;
    type RouterExtraSettings = AwsRouterExtraSettings;

    fn cloud_provider() -> Kind {
        Kind::Aws
    }

    fn short_name() -> &'static str {
        "AWS"
    }

    fn full_name() -> &'static str {
        "Amazon Web Service"
    }

    fn registry_short_name() -> &'static str {
        "ECR"
    }

    fn registry_full_name() -> &'static str {
        "Elastic Container Registry"
    }

    fn lib_directory_name() -> &'static str {
        "aws"
    }
}

impl AWS {}

#[derive(Clone, Eq, PartialEq)]
pub enum AwsStorageType {
    GP2,
}

impl ToCloudProviderFormat for AwsStorageType {
    fn to_cloud_provider_format(&self) -> &str {
        match self {
            AwsStorageType::GP2 => "gp2",
        }
    }
}

impl Display for AwsStorageType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            AwsStorageType::GP2 => write!(f, "GP2"),
        }
    }
}

impl AwsStorageType {
    pub fn to_k8s_storage_class(&self) -> String {
        match self {
            AwsStorageType::GP2 => "aws-ebs-gp2-0",
        }
        .to_string()
    }
}

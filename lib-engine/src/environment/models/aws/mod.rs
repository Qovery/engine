use std::fmt::Display;
use std::fmt::Formatter;

use crate::environment::models::ToCloudProviderFormat;
use crate::environment::models::types::AWS;
use crate::environment::models::types::CloudProvider;
use crate::infrastructure::models::cloud_provider::Kind;
use crate::io_models::database::DiskIOPS;

mod database;
mod database_utils;
mod job;
mod router;
mod terraform_service;

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

#[derive(Clone, Eq, PartialEq)]
pub enum AwsStorageType {
    GP2,
    // GP3 { disk_iops: DiskIOPS }, <= Not supported yet, but to be added in the future including IOPS
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

    pub fn get_disk_iops(&self) -> DiskIOPS {
        match self {
            AwsStorageType::GP2 => DiskIOPS::Default,
            // AwsStorageType::GP3 { disk_iops } => *disk_iops, <= Not supported yet, but to be added in the future including IOPS
        }
    }
}

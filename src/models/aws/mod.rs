mod application;
mod container;
mod database;
mod database_utils;
mod job;
mod router;

use crate::cloud_provider::Kind;
use std::fmt::Display;
use std::fmt::Formatter;

use crate::models::types::CloudProvider;
use crate::models::types::AWS;

pub struct AwsAppExtraSettings {}
pub struct AwsDbExtraSettings {}
pub struct AwsRouterExtraSettings {}

impl CloudProvider for AWS {
    type AppExtraSettings = AwsAppExtraSettings;
    type DbExtraSettings = AwsDbExtraSettings;
    type RouterExtraSettings = AwsRouterExtraSettings;
    type StorageTypes = AwsStorageType;

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

#[derive(Clone, Eq, PartialEq, Hash)]
pub enum AwsStorageType {
    SC1,
    ST1,
    GP2,
    IO1,
}

impl Display for AwsStorageType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            AwsStorageType::SC1 => write!(f, "SC1"),
            AwsStorageType::ST1 => write!(f, "ST1"),
            AwsStorageType::GP2 => write!(f, "GP2"),
            AwsStorageType::IO1 => write!(f, "IO1"),
        }
    }
}

impl AwsStorageType {
    pub fn to_k8s_storage_class(&self) -> String {
        match self {
            AwsStorageType::SC1 => "aws-ebs-sc1-0",
            AwsStorageType::ST1 => "aws-ebs-st1-0",
            AwsStorageType::GP2 => "aws-ebs-gp2-0",
            AwsStorageType::IO1 => "aws-ebs-io1-0",
        }
        .to_string()
    }
}

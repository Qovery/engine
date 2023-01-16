mod application;
mod container;
mod database;
mod database_utils;
mod job;
mod router;

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

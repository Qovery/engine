pub mod application;
pub mod router;

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

    fn helm_directory_name() -> &'static str {
        "aws"
    }
}

#[derive(Clone, Eq, PartialEq, Hash)]
pub enum AwsStorageType {
    SC1,
    ST1,
    GP2,
    IO1,
}

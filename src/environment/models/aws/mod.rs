use crate::environment::models::types::AWS;
use crate::environment::models::types::CloudProvider;
use crate::infrastructure::models::cloud_provider::Kind;

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

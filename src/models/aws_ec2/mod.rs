mod database;
mod database_utils;
mod job;
mod router;

use crate::cloud_provider::Kind;

use crate::models::types::{AWSEc2, CloudProvider};

pub struct AwsEc2AppExtraSettings {}
pub struct AwsEc2DbExtraSettings {}
pub struct AwsEc2RouterExtraSettings {}

impl CloudProvider for AWSEc2 {
    type AppExtraSettings = AwsEc2AppExtraSettings;
    type DbExtraSettings = AwsEc2DbExtraSettings;
    type RouterExtraSettings = AwsEc2RouterExtraSettings;
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
        "aws-ec2"
    }
}

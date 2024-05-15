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

    fn loadbalancer_l4_annotations() -> &'static [(&'static str, &'static str)] {
        &[("service.beta.kubernetes.io/aws-load-balancer-type", "nlb")]
    }
}

impl AWS {}

#[derive(Clone, Eq, PartialEq)]
pub enum AwsStorageType {
    GP2,
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

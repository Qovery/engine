mod application;
mod container;
mod database;
mod database_utils;
mod job;
mod router;

use std::fmt::{Display, Formatter};

use crate::models::types::{AWSEc2, CloudProvider};

pub struct AwsEc2AppExtraSettings {}
pub struct AwsEc2DbExtraSettings {}
pub struct AwsEc2RouterExtraSettings {}

impl CloudProvider for AWSEc2 {
    type AppExtraSettings = AwsEc2AppExtraSettings;
    type DbExtraSettings = AwsEc2DbExtraSettings;
    type RouterExtraSettings = AwsEc2RouterExtraSettings;
    type StorageTypes = AwsEc2StorageType;

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

#[derive(Clone, Eq, PartialEq, Hash)]
pub enum AwsEc2StorageType {
    SC1,
    ST1,
    GP2,
    IO1,
}

impl Display for AwsEc2StorageType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            AwsEc2StorageType::SC1 => write!(f, "SC1"),
            AwsEc2StorageType::ST1 => write!(f, "ST1"),
            AwsEc2StorageType::GP2 => write!(f, "GP2"),
            AwsEc2StorageType::IO1 => write!(f, "IO1"),
        }
    }
}

impl AwsEc2StorageType {
    pub fn to_k8s_storage_class(&self) -> String {
        match self {
            AwsEc2StorageType::SC1 => "aws-ebs-sc1-0",
            AwsEc2StorageType::ST1 => "aws-ebs-st1-0",
            AwsEc2StorageType::GP2 => "aws-ebs-gp2-0",
            AwsEc2StorageType::IO1 => "aws-ebs-io1-0",
        }
        .to_string()
    }
}

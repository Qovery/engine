use crate::{cloud_provider::Kind as KindModel, errors::EngineError, events::EventDetails};
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str;
use std::time::Duration;

pub const CLOUDWATCH_RETENTION_DAYS: &[u32] = &[
    0, 1, 3, 5, 7, 14, 30, 60, 90, 120, 150, 180, 365, 400, 545, 731, 1827, 2192, 2557, 2922, 3288, 3653,
];

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Aws,
    Do,
    Scw,
}

impl From<KindModel> for Kind {
    fn from(kind: KindModel) -> Self {
        match kind {
            KindModel::Aws => Kind::Aws,
            KindModel::Scw => Kind::Scw,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "lowercase")]
pub enum AwsEc2MetadataImds {
    Required,
    Optional,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct ClusterAdvancedSettings {
    #[serde(alias = "load_balancer.size")]
    pub load_balancer_size: String,
    #[serde(alias = "registry.image_retention_time")]
    pub registry_image_retention_time_sec: u32,
    #[serde(alias = "pleco.resources_ttl")]
    pub pleco_resources_ttl: i32,
    #[serde(alias = "loki.log_retention_in_week")]
    pub loki_log_retention_in_week: u32,
    #[serde(alias = "aws.iam.enable_admin_group_sync")]
    pub aws_iam_user_mapper_group_enabled: bool,
    #[serde(alias = "aws.iam.admin_group")]
    pub aws_iam_user_mapper_group_name: Option<String>,
    #[serde(alias = "aws.iam.enable_sso")]
    pub aws_iam_user_mapper_sso_enabled: bool,
    #[serde(alias = "aws.iam.sso_role_arn")]
    pub aws_iam_user_mapper_sso_role_arn: Option<String>,
    #[serde(alias = "aws.eks.ec2.metadata_imds")]
    pub aws_eks_ec2_metadata_imds: AwsEc2MetadataImds,
    #[serde(alias = "aws.vpc.enable_s3_flow_logs")]
    pub aws_vpc_enable_flow_logs: bool,
    #[serde(alias = "aws.vpc.flow_logs_retention_days")]
    pub aws_vpc_flow_logs_retention_days: u32,
    #[serde(alias = "aws.cloudwatch.eks_logs_retention_days")]
    pub aws_cloudwatch_eks_logs_retention_days: u32,
    #[serde(alias = "cloud_provider.container_registry.tags")]
    pub cloud_provider_container_registry_tags: HashMap<String, String>,
    #[serde(alias = "database.postgresql.deny_public_access")]
    pub database_postgresql_deny_public_access: bool,
    #[serde(alias = "database.postgresql.allowed_cidrs")]
    pub database_postgresql_allowed_cidrs: Vec<String>,
    #[serde(alias = "database.mysql.deny_public_access")]
    pub database_mysql_deny_public_access: bool,
    #[serde(alias = "database.mysql.allowed_cidrs")]
    pub database_mysql_allowed_cidrs: Vec<String>,
    #[serde(alias = "database.redis.deny_public_access")]
    pub database_redis_deny_public_access: bool,
    #[serde(alias = "database.redis.allowed_cidrs")]
    pub database_redis_allowed_cidrs: Vec<String>,
    #[serde(alias = "database.mongodb.deny_public_access")]
    pub database_mongodb_deny_public_access: bool,
    #[serde(alias = "database.mongodb.allowed_cidrs")]
    pub database_mongodb_allowed_cidrs: Vec<String>,
}

impl Default for ClusterAdvancedSettings {
    fn default() -> Self {
        let default_database_cirds = vec!["0.0.0.0/0".to_string()];
        ClusterAdvancedSettings {
            load_balancer_size: "lb-s".to_string(),
            registry_image_retention_time_sec: 31536000,
            pleco_resources_ttl: -1,
            loki_log_retention_in_week: 12,
            aws_iam_user_mapper_group_enabled: true,
            aws_iam_user_mapper_group_name: Some("Admins".to_string()),
            aws_iam_user_mapper_sso_enabled: false,
            aws_iam_user_mapper_sso_role_arn: None,
            cloud_provider_container_registry_tags: HashMap::new(),
            aws_eks_ec2_metadata_imds: AwsEc2MetadataImds::Optional,
            aws_vpc_enable_flow_logs: false,
            aws_vpc_flow_logs_retention_days: 365,
            aws_cloudwatch_eks_logs_retention_days: 90,
            database_postgresql_deny_public_access: false,
            database_postgresql_allowed_cidrs: default_database_cirds.clone(),
            database_mysql_deny_public_access: false,
            database_mysql_allowed_cidrs: default_database_cirds.clone(),
            database_redis_deny_public_access: false,
            database_redis_allowed_cidrs: default_database_cirds.clone(),
            database_mongodb_deny_public_access: false,
            database_mongodb_allowed_cidrs: default_database_cirds,
        }
    }
}

impl ClusterAdvancedSettings {
    pub fn validate(&self, event_details: EventDetails) -> Result<(), Box<EngineError>> {
        // AWS Cloudwatch EKS logs retention days
        if !validate_aws_cloudwatch_eks_logs_retention_days(self.aws_cloudwatch_eks_logs_retention_days) {
            return Err(Box::new(EngineError::new_aws_wrong_cloudwatch_retention_configuration(
                event_details,
                self.aws_cloudwatch_eks_logs_retention_days,
                CLOUDWATCH_RETENTION_DAYS,
            )));
        }

        Ok(())
    }

    pub fn resource_ttl(&self) -> Option<Duration> {
        if self.pleco_resources_ttl >= 0 {
            Some(Duration::new(self.pleco_resources_ttl as u64, 0))
        } else {
            None
        }
    }
}

// AWS
fn validate_aws_cloudwatch_eks_logs_retention_days(days: u32) -> bool {
    CLOUDWATCH_RETENTION_DAYS.contains(&days)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CustomerHelmChartsOverrideEncoded {
    pub chart_name: String,
    pub b64_chart_values: String,
}

impl CustomerHelmChartsOverrideEncoded {
    pub fn to_decoded_customer_helm_chart_override(b64_chart_values: String) -> Result<String, String> {
        match base64::decode(b64_chart_values) {
            Ok(x) => match str::from_utf8(&x) {
                Ok(content) => Ok(content.to_string()),
                Err(e) => Err(e.to_string()),
            },
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use crate::{
        cloud_provider::io::validate_aws_cloudwatch_eks_logs_retention_days,
        events::{EventDetails, Stage, Transmitter},
        io_models::QoveryIdentifier,
    };

    #[test]
    // avoid human mistakes and check defaults values at compile time
    fn ensure_cluster_advanced_settings_defaults_are_valid() {
        let settings = super::ClusterAdvancedSettings::default();
        let event_details = EventDetails::new(
            None,
            QoveryIdentifier::default(),
            QoveryIdentifier::default(),
            "".to_string(),
            Stage::Infrastructure(crate::events::InfrastructureStep::ValidateApiInput),
            Transmitter::Kubernetes(Uuid::new_v4(), "".to_string()),
        );
        assert!(settings.validate(event_details).is_ok());
    }

    #[test]
    fn cloudwatch_eks_log_retention_days() {
        assert!(validate_aws_cloudwatch_eks_logs_retention_days(0));
        assert!(validate_aws_cloudwatch_eks_logs_retention_days(90));
        assert!(!validate_aws_cloudwatch_eks_logs_retention_days(2));
    }
}

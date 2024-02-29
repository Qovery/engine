use crate::{cloud_provider::Kind as KindModel, errors::EngineError, events::EventDetails};
use base64::engine::general_purpose;
use base64::Engine;
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str;
use std::time::Duration;

pub const CLOUDWATCH_RETENTION_DAYS: &[u32] = &[
    0, 1, 3, 5, 7, 14, 30, 60, 90, 120, 150, 180, 365, 400, 545, 731, 1827, 2192, 2557, 2922, 3288, 3653,
];

fn default_registry_mirroring_mode() -> RegistryMirroringMode {
    RegistryMirroringMode::Service
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Aws,
    Do,
    Scw,
    Gcp,
    SelfManaged,
}

impl From<KindModel> for Kind {
    fn from(kind: KindModel) -> Self {
        match kind {
            KindModel::Aws => Kind::Aws,
            KindModel::Scw => Kind::Scw,
            KindModel::Gcp => Kind::Gcp,
            KindModel::SelfManaged => Kind::SelfManaged,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "lowercase")]
pub enum AwsEc2MetadataImds {
    Required,
    Optional,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum RegistryMirroringMode {
    #[serde(alias = "cluster", alias = "CLUSTER")]
    Cluster,
    #[serde(alias = "service", alias = "SERVICE")]
    #[serde(other)]
    Service,
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
    #[serde(alias = "aws.enable_karpenter")]
    pub aws_enable_karpenter: bool,
    #[serde(alias = "aws.karpenter.max_node_drain_in_sec")]
    pub aws_karpenter_max_node_drain_in_sec: Option<i32>,
    #[serde(alias = "aws.eks.ec2.metadata_imds")]
    pub aws_eks_ec2_metadata_imds: AwsEc2MetadataImds,
    #[serde(alias = "aws.vpc.enable_s3_flow_logs")]
    pub aws_vpc_enable_flow_logs: bool,
    #[serde(alias = "aws.vpc.flow_logs_retention_days")]
    pub aws_vpc_flow_logs_retention_days: u32,
    #[serde(alias = "aws.cloudwatch.eks_logs_retention_days")]
    pub aws_cloudwatch_eks_logs_retention_days: u32,
    #[serde(alias = "aws.eks.encrypt_secrets_kms_key_arn", default)]
    pub aws_eks_encrypt_secrets_kms_key_arn: String,
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
    #[serde(alias = "registry.mirroring_mode", default = "default_registry_mirroring_mode")]
    pub registry_mirroring_mode: RegistryMirroringMode,
    #[serde(alias = "nginx.vcpu.request_in_milli_cpu")]
    pub nginx_vcpu_request_in_milli_cpu: u32,
    #[serde(alias = "nginx.vcpu.limit_in_milli_cpu")]
    pub nginx_vcpu_limit_in_milli_cpu: u32,
    #[serde(alias = "nginx.memory.request_in_mib")]
    pub nginx_memory_request_in_mib: u32,
    #[serde(alias = "nginx.memory.limit_in_mib")]
    pub nginx_memory_limit_in_mib: u32,
    #[serde(alias = "nginx.hpa.cpu_utilization_percentage_threshold")]
    pub nginx_hpa_cpu_utilization_percentage_threshold: u32,
    #[serde(alias = "nginx.hpa.min_number_instances")]
    pub nginx_hpa_min_number_instances: u32,
    #[serde(alias = "nginx.hpa.max_number_instances")]
    pub nginx_hpa_max_number_instances: u32,
    #[serde(alias = "scaleway.enable_private_network_migration")]
    pub scaleway_enable_private_network_migration: bool,
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
            registry_mirroring_mode: RegistryMirroringMode::Service,
            nginx_vcpu_request_in_milli_cpu: 100,
            nginx_vcpu_limit_in_milli_cpu: 500,
            nginx_memory_request_in_mib: 768,
            nginx_memory_limit_in_mib: 768,
            nginx_hpa_cpu_utilization_percentage_threshold: 50,
            nginx_hpa_min_number_instances: 2,
            nginx_hpa_max_number_instances: 25,
            scaleway_enable_private_network_migration: false,
            aws_eks_encrypt_secrets_kms_key_arn: "".to_string(),
            aws_enable_karpenter: false,
            aws_karpenter_max_node_drain_in_sec: None,
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
        match general_purpose::STANDARD.decode(b64_chart_values) {
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

    use crate::cloud_provider::io::{ClusterAdvancedSettings, RegistryMirroringMode};
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

    #[test]
    fn test_registry_mirroring_mode_deserialization() {
        struct TestCase {
            input: String,
            expected: RegistryMirroringMode,
        }

        let test_cases = vec![
            TestCase {
                input: "Service".to_string(),
                expected: RegistryMirroringMode::Service,
            },
            TestCase {
                input: "service".to_string(),
                expected: RegistryMirroringMode::Service,
            },
            TestCase {
                input: "SERVICE".to_string(),
                expected: RegistryMirroringMode::Service,
            },
            TestCase {
                input: "Cluster".to_string(),
                expected: RegistryMirroringMode::Cluster,
            },
            TestCase {
                input: "cluster".to_string(),
                expected: RegistryMirroringMode::Cluster,
            },
            TestCase {
                input: "CLUSTER".to_string(),
                expected: RegistryMirroringMode::Cluster,
            },
            TestCase {
                input: "TOTO".to_string(),
                expected: RegistryMirroringMode::Service,
            },
        ];

        for tc in test_cases {
            let data = format!(
                r#"
        {{
            "registry.mirroring_mode": "{}"
        }}"#,
                tc.input
            );

            let cluster_advanced_settings: ClusterAdvancedSettings = serde_json::from_str(data.as_str()).unwrap();
            assert_eq!(cluster_advanced_settings.registry_mirroring_mode, tc.expected);
        }
    }

    #[test]
    fn test_default_values_for_nginx() {
        let data = r#" {}"#;
        let cluster_advanced_settings: ClusterAdvancedSettings = serde_json::from_str(data).unwrap();
        assert_eq!(cluster_advanced_settings.nginx_vcpu_request_in_milli_cpu, 100);
        assert_eq!(cluster_advanced_settings.nginx_vcpu_limit_in_milli_cpu, 500);
        assert_eq!(cluster_advanced_settings.nginx_memory_request_in_mib, 768);
        assert_eq!(cluster_advanced_settings.nginx_memory_limit_in_mib, 768);
        assert_eq!(cluster_advanced_settings.nginx_hpa_cpu_utilization_percentage_threshold, 50);
        assert_eq!(cluster_advanced_settings.nginx_hpa_min_number_instances, 2);
        assert_eq!(cluster_advanced_settings.nginx_hpa_max_number_instances, 25);
    }

    #[test]
    fn test_nginx_deserialization() {
        let nginx_vcpu_request_in_milli_cpu = 155;
        let nginx_hpa_cpu_utilization_percentage_threshold = 75;
        let data = format!(
            r#"
        {{
            "nginx.vcpu.request_in_milli_cpu": {},
            "nginx.hpa.cpu_utilization_percentage_threshold": {}
        }}"#,
            nginx_vcpu_request_in_milli_cpu, nginx_hpa_cpu_utilization_percentage_threshold
        );
        let cluster_advanced_settings: ClusterAdvancedSettings = serde_json::from_str(data.as_str()).unwrap();
        assert_eq!(
            cluster_advanced_settings.nginx_vcpu_request_in_milli_cpu,
            nginx_vcpu_request_in_milli_cpu
        );
        assert_eq!(cluster_advanced_settings.nginx_vcpu_limit_in_milli_cpu, 500);
        assert_eq!(cluster_advanced_settings.nginx_memory_request_in_mib, 768);
        assert_eq!(cluster_advanced_settings.nginx_memory_limit_in_mib, 768);
        assert_eq!(
            cluster_advanced_settings.nginx_hpa_cpu_utilization_percentage_threshold,
            nginx_hpa_cpu_utilization_percentage_threshold
        );
        assert_eq!(cluster_advanced_settings.nginx_hpa_min_number_instances, 2);
        assert_eq!(cluster_advanced_settings.nginx_hpa_max_number_instances, 25);
    }
}

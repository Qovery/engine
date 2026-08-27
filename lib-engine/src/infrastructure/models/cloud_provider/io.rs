use crate::environment::models::domain::ToTerraformString;
use crate::environment::models::types::Percentage;
use crate::infrastructure::helm_charts::nginx_ingress_chart::{
    LogFormatEscaping as LogFormatEscapingModel, NginxConfigurationSnippet as NginxConfigurationSnippetModel,
    NginxHttpSnippet as NginxHttpSnippetModel, NginxLimitRequestStatusCode as NginxLimitRequestStatusCodeModel,
    NginxServerSnippet as NginxServerSnippetModel,
};
use crate::infrastructure::helm_charts::qovery_cluster_gateway_chart::{
    EnvoyClientValidationCaCertificate as EnvoyClientValidationCaCertificateModel,
    EnvoyGatewayApiPathEscapedSlashesAction as EnvoyGatewayApiPathEscapedSlashesActionModel,
};
use crate::infrastructure::models::cloud_provider::Kind as KindModel;
use crate::infrastructure::models::cloud_provider::aws::ec2_ami::Ec2Ami as Ec2AmiModel;
use crate::infrastructure::models::cluster_profile::ClusterProfile as ClusterProfileModel;
use crate::io_models::loki::LokiDeploymentMode;
use crate::io_models::models::StorageClass as StorageClassModel;
use crate::io_models::types::gateway_api_retry_triggers::GatewayApiRetryTrigger;
use crate::{errors::EngineError, events::EventDetails};
use base64::Engine;
use base64::engine::general_purpose;
use ipnet::IpNet;
use reqwest::StatusCode;
use serde::Deserialize as SerdeDeserialize;
use serde_derive::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt::Display;
use std::str;
use std::time::Duration;
use thiserror::Error;

pub const CLOUDWATCH_RETENTION_DAYS: &[u32] = &[
    0, 1, 3, 5, 7, 14, 30, 60, 90, 120, 150, 180, 365, 400, 545, 731, 1827, 2192, 2557, 2922, 3288, 3653,
];
const ENVOY_CLIENT_VALIDATION_SECRET_NAME_PREFIX: &str = "envoy-client-validation-";
const ENVOY_CLIENT_VALIDATION_MAX_CA_CERTIFICATES: usize = 8;

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum InputError {
    #[error("Invalid input field value for `{field_name}`: `{message}`")]
    InvalidInputFieldValue { field_name: String, message: String },
}

fn default_registry_mirroring_mode() -> RegistryMirroringMode {
    RegistryMirroringMode::Service
}

fn default_nginx_controller_log_format_escaping() -> LogFormatEscaping {
    LogFormatEscaping::Default
}

fn default_aws_eks_ec2_ami() -> Ec2Ami {
    Ec2Ami::AmazonLinux2023
}

fn default_aws_alb_controller_replicas() -> u32 {
    2
}

fn default_envoy_gateway_controller_replicas() -> u32 {
    2
}

fn default_envoy_gateway_api_path_escaped_slashes_action() -> EnvoyGatewayApiPathEscapedSlashesAction {
    EnvoyGatewayApiPathEscapedSlashesAction::UnescapeAndRedirect
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub enum EnvoyGatewayApiPathEscapedSlashesAction {
    #[serde(rename = "KeepUnchanged")]
    KeepUnchanged, // Preserve %2F as-is in the upstream path.
    #[serde(rename = "RejectRequest")]
    RejectRequest, // Reject requests containing escaped slashes.
    #[serde(rename = "UnescapeAndForward")]
    UnescapeAndForward, // Decode %2F to / and forward upstream.
    #[default]
    #[serde(rename = "UnescapeAndRedirect")]
    UnescapeAndRedirect, // Decode %2F and redirect client to normalized path.
}

impl Display for EnvoyGatewayApiPathEscapedSlashesAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            EnvoyGatewayApiPathEscapedSlashesAction::KeepUnchanged => "KeepUnchanged",
            EnvoyGatewayApiPathEscapedSlashesAction::RejectRequest => "RejectRequest",
            EnvoyGatewayApiPathEscapedSlashesAction::UnescapeAndForward => "UnescapeAndForward",
            EnvoyGatewayApiPathEscapedSlashesAction::UnescapeAndRedirect => "UnescapeAndRedirect",
        };
        write!(f, "{value}")
    }
}

impl EnvoyGatewayApiPathEscapedSlashesAction {
    pub fn to_model(&self) -> EnvoyGatewayApiPathEscapedSlashesActionModel {
        match self {
            EnvoyGatewayApiPathEscapedSlashesAction::KeepUnchanged => {
                EnvoyGatewayApiPathEscapedSlashesActionModel::KeepUnchanged
            }
            EnvoyGatewayApiPathEscapedSlashesAction::RejectRequest => {
                EnvoyGatewayApiPathEscapedSlashesActionModel::RejectRequest
            }
            EnvoyGatewayApiPathEscapedSlashesAction::UnescapeAndForward => {
                EnvoyGatewayApiPathEscapedSlashesActionModel::UnescapeAndForward
            }
            EnvoyGatewayApiPathEscapedSlashesAction::UnescapeAndRedirect => {
                EnvoyGatewayApiPathEscapedSlashesActionModel::UnescapeAndRedirect
            }
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Aws,
    Azure,
    Do,
    Scw,
    Gcp,
    SelfManaged,
}

impl From<KindModel> for Kind {
    fn from(kind: KindModel) -> Self {
        match kind {
            KindModel::Aws => Kind::Aws,
            KindModel::Azure => Kind::Azure,
            KindModel::Scw => Kind::Scw,
            KindModel::Gcp => Kind::Gcp,
            KindModel::OnPremise => Kind::SelfManaged,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "lowercase")]
pub enum AwsEc2MetadataImds {
    Required,
    Optional,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Copy, Default)]
pub enum AwsAlbLoadBalancerScheme {
    #[default]
    #[serde(rename = "internet-facing")]
    InternetFacing,
    #[serde(rename = "internal")]
    Internal,
}

impl Display for AwsAlbLoadBalancerScheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            AwsAlbLoadBalancerScheme::InternetFacing => "internet-facing",
            AwsAlbLoadBalancerScheme::Internal => "internal",
        };
        write!(f, "{}", str)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum EfsThroughputMode {
    #[serde(rename = "elastic")]
    Elastic,
    #[serde(rename = "bursting")]
    Bursting,
}

impl ToTerraformString for EfsThroughputMode {
    fn to_terraform_format_string(&self) -> String {
        match self {
            EfsThroughputMode::Elastic => "elastic".to_string(),
            EfsThroughputMode::Bursting => "bursting".to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum EfsPerformanceMode {
    #[serde(rename = "generalPurpose")]
    GeneralPurpose,
    #[serde(rename = "maxIO")]
    MaxIO,
}

impl ToTerraformString for EfsPerformanceMode {
    fn to_terraform_format_string(&self) -> String {
        match self {
            EfsPerformanceMode::GeneralPurpose => "generalPurpose".to_string(),
            EfsPerformanceMode::MaxIO => "maxIO".to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum EfsTransitionToIa {
    #[serde(rename = "")]
    Disabled,
    #[serde(rename = "AFTER_1_DAY")]
    After1Day,
    #[serde(rename = "AFTER_7_DAYS")]
    After7Days,
    #[serde(rename = "AFTER_14_DAYS")]
    After14Days,
    #[serde(rename = "AFTER_30_DAYS")]
    After30Days,
    #[serde(rename = "AFTER_60_DAYS")]
    After60Days,
    #[serde(rename = "AFTER_90_DAYS")]
    After90Days,
}

impl ToTerraformString for EfsTransitionToIa {
    fn to_terraform_format_string(&self) -> String {
        match self {
            EfsTransitionToIa::Disabled => "".to_string(),
            EfsTransitionToIa::After1Day => "AFTER_1_DAY".to_string(),
            EfsTransitionToIa::After7Days => "AFTER_7_DAYS".to_string(),
            EfsTransitionToIa::After14Days => "AFTER_14_DAYS".to_string(),
            EfsTransitionToIa::After30Days => "AFTER_30_DAYS".to_string(),
            EfsTransitionToIa::After60Days => "AFTER_60_DAYS".to_string(),
            EfsTransitionToIa::After90Days => "AFTER_90_DAYS".to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StorageClass(String);

impl Display for StorageClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl StorageClass {
    pub fn to_model(&self) -> StorageClassModel {
        StorageClassModel(self.0.to_string())
    }
}

impl From<StorageClassModel> for StorageClass {
    fn from(storage_class: StorageClassModel) -> Self {
        StorageClass(storage_class.0.to_string())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum RegistryMirroringMode {
    #[serde(alias = "cluster", alias = "CLUSTER")]
    Cluster,
    #[serde(alias = "service", alias = "SERVICE")]
    #[serde(other)]
    Service,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum LogFormatEscaping {
    #[serde(alias = "default")]
    Default,
    #[serde(alias = "none")]
    None,
    #[serde(alias = "json", alias = "Json")]
    JSON,
}

impl LogFormatEscaping {
    pub fn to_model(&self) -> LogFormatEscapingModel {
        match &self {
            LogFormatEscaping::Default => LogFormatEscapingModel::Default,
            LogFormatEscaping::None => LogFormatEscapingModel::None,
            LogFormatEscaping::JSON => LogFormatEscapingModel::JSON,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct NginxHttpSnippet(String);

impl NginxHttpSnippet {
    pub fn to_model(&self) -> NginxHttpSnippetModel {
        NginxHttpSnippetModel::new(self.0.to_string())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub struct NginxServerSnippet(String);

impl NginxServerSnippet {
    pub fn to_model(&self) -> NginxServerSnippetModel {
        NginxServerSnippetModel::new(self.0.to_string())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub struct NginxConfigurationSnippet(String);

impl NginxConfigurationSnippet {
    pub fn to_model(&self) -> NginxConfigurationSnippetModel {
        NginxConfigurationSnippetModel::new(self.0.to_string())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub struct NginxLimitRequestStatusCode(u16);

impl NginxLimitRequestStatusCode {
    pub fn to_model(&self) -> Result<NginxLimitRequestStatusCodeModel, InputError> {
        let status_code = StatusCode::from_u16(self.0).map_err(|_e| InputError::InvalidInputFieldValue {
            field_name: "nginx.controller.limit_request_status_code".to_string(),
            message: "Should be a proper HTTP status code".to_string(),
        })?;
        Ok(NginxLimitRequestStatusCodeModel::new(status_code))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Ec2Ami {
    AmazonLinux2,
    AmazonLinux2023,
    Bottlerocket,
    Custom(String),
}

impl Ec2Ami {
    pub fn to_model(&self) -> Ec2AmiModel {
        match self {
            Ec2Ami::AmazonLinux2 => Ec2AmiModel::AmazonLinux2,
            Ec2Ami::AmazonLinux2023 => Ec2AmiModel::AmazonLinux2023,
            Ec2Ami::Bottlerocket => Ec2AmiModel::Bottlerocket,
            Ec2Ami::Custom(v) => Ec2AmiModel::Custom(v.clone()),
        }
    }
}

impl serde::Serialize for Ec2Ami {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Ec2Ami::AmazonLinux2 => serializer.serialize_str("AmazonLinux2"),
            Ec2Ami::AmazonLinux2023 => serializer.serialize_str("AmazonLinux2023"),
            Ec2Ami::Bottlerocket => serializer.serialize_str("Bottlerocket"),
            Ec2Ami::Custom(v) => serializer.serialize_str(v),
        }
    }
}

impl<'de> serde::Deserialize<'de> for Ec2Ami {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = <String as SerdeDeserialize>::deserialize(deserializer)?;
        match s.as_str() {
            "AmazonLinux2" => Ok(Ec2Ami::AmazonLinux2),
            "AmazonLinux2023" => Ok(Ec2Ami::AmazonLinux2023),
            "Bottlerocket" => Ok(Ec2Ami::Bottlerocket),
            other => {
                // Validate: must be an AMI ID (ami-xxx) or a name pattern (non-empty)
                if other.is_empty() {
                    return Err(serde::de::Error::custom("custom AMI value cannot be empty"));
                }
                Ok(Ec2Ami::Custom(other.to_string()))
            }
        }
    }
}

#[derive(Copy, Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub enum ClusterProfile {
    /// Node count: 3-5 nodes
    /// Total cluster capacity: 12-20 vCPUs, 24-40 GB RAM
    /// Per-node size: 2-4 vCPUs, 4-8 GB RAM
    /// Workload characteristics: Dev/test environments, low-traffic applications
    /// Concurrent pods: ~50-100
    /// Use cases: Development, staging, small internal tools
    Small,
    /// Node count: 6-10 nodes
    /// Total cluster capacity: 48-80 vCPUs, 96-160 GB RAM
    /// Per-node size: 4-8 vCPUs, 8-16 GB RAM
    /// Workload characteristics: Production workloads with moderate traffic
    /// Concurrent pods: ~200-400
    /// Use cases: Small to medium production apps, multi-tenant dev environments
    Medium,
    /// Node count: 11-20 nodes
    /// Total cluster capacity: 176-320 vCPUs, 352-640 GB RAM
    /// Per-node size: 8-16 vCPUs, 16-32 GB RAM
    /// Workload characteristics: High-traffic production workloads
    /// Concurrent pods: ~500-1000
    /// Use cases: Enterprise production applications, microservices architectures
    Large,
    /// Node count: 20+ nodes
    /// Total cluster capacity: 400+ vCPUs, 800+ GB RAM
    /// Per-node size: 16-32+ vCPUs, 32-64+ GB RAM
    /// Workload characteristics: Mission-critical, high-scale applications
    /// Concurrent pods: 1000+
    /// Use cases: Large-scale production, ML/AI workloads, data processing
    ExtraLarge,
}

impl ClusterProfile {
    pub fn to_model(&self) -> ClusterProfileModel {
        match self {
            ClusterProfile::Small => ClusterProfileModel::Small,
            ClusterProfile::Medium => ClusterProfileModel::Medium,
            ClusterProfile::Large => ClusterProfileModel::Large,
            ClusterProfile::ExtraLarge => ClusterProfileModel::ExtraLarge,
        }
    }
}

fn default_cluster_profile() -> ClusterProfile {
    ClusterProfile::Medium
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Certificate {
    pub tls_crt: String,
    pub tls_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LoadBalancerIpAllocationId(String);

impl LoadBalancerIpAllocationId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for LoadBalancerIpAllocationId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for LoadBalancerIpAllocationId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvoyClientValidationCaCertificate {
    pub name: String,
    pub ca_crt: String,
}

impl EnvoyClientValidationCaCertificate {
    pub fn to_model(&self) -> Result<EnvoyClientValidationCaCertificateModel, InputError> {
        let field_name = "envoy.client_validation.ca_certificates".to_string();
        let name = self.name.trim();
        let ca_crt = self.ca_crt.trim();

        if name.is_empty() {
            return Err(InputError::InvalidInputFieldValue {
                field_name,
                message: "certificate `name` must not be empty".to_string(),
            });
        }

        if !is_valid_kubernetes_secret_name(name) {
            return Err(InputError::InvalidInputFieldValue {
                field_name,
                message: format!(
                    "certificate `{name}` has an invalid `name`: expected a valid Kubernetes DNS-1123 subdomain"
                ),
            });
        }

        let secret_name = envoy_client_validation_secret_name(name);
        if !is_valid_kubernetes_secret_name(&secret_name) {
            return Err(InputError::InvalidInputFieldValue {
                field_name,
                message: format!(
                    "certificate `{name}` has an invalid `name`: final secret name `{secret_name}` must be a valid Kubernetes DNS-1123 subdomain"
                ),
            });
        }

        if ca_crt.is_empty() {
            return Err(InputError::InvalidInputFieldValue {
                field_name,
                message: format!("certificate `{name}` has an empty `ca_crt`"),
            });
        }

        Ok(EnvoyClientValidationCaCertificateModel {
            name: secret_name,
            namespace: crate::helm::HelmChartNamespaces::Qovery,
            ca_crt: ca_crt.to_string(),
        })
    }
}

fn envoy_client_validation_secret_name(name: &str) -> String {
    format!("{ENVOY_CLIENT_VALIDATION_SECRET_NAME_PREFIX}{name}")
}

fn is_valid_kubernetes_secret_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 253 {
        return false;
    }

    let bytes = name.as_bytes();
    let is_lowercase_alphanumeric = |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit();
    let is_dns_subdomain_char = |byte: u8| is_lowercase_alphanumeric(byte) || byte == b'-' || byte == b'.';

    if !is_lowercase_alphanumeric(bytes[0]) || !is_lowercase_alphanumeric(bytes[bytes.len() - 1]) {
        return false;
    }

    bytes.iter().all(|byte| is_dns_subdomain_char(*byte))
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct ClusterAdvancedSettings {
    #[serde(alias = "cluster.profile", default = "default_cluster_profile")]
    pub cluster_profile: ClusterProfile,
    #[serde(alias = "load_balancer.size")]
    pub load_balancer_size: String,
    #[serde(
        alias = "k8s.gateway.load_balancer_source_ranges",
        alias = "aws.eks.alb_controller.load_balancer_source_ranges",
        default
    )]
    pub load_balancer_source_ranges: Vec<IpNet>,
    #[serde(alias = "k8s.gateway.load_balancer_ip_allocation_ids", default)]
    pub load_balancer_ip_allocation_ids: Option<Vec<LoadBalancerIpAllocationId>>,
    #[serde(alias = "registry.image_retention_time")]
    pub registry_image_retention_time_sec: u32,
    #[serde(alias = "pleco.resources_ttl")]
    pub pleco_resources_ttl: i32,
    #[serde(alias = "loki.log_retention_in_week")]
    pub loki_log_retention_in_week: u32,
    #[serde(alias = "loki.deployment_mode", default)]
    pub loki_deployment_mode: LokiDeploymentMode,
    #[serde(alias = "loki.single_binary.cpu_request_m")]
    pub loki_single_binary_cpu_request_m: Option<u32>,
    #[serde(alias = "loki.single_binary.cpu_limit_m")]
    pub loki_single_binary_cpu_limit_m: Option<u32>,
    #[serde(alias = "loki.single_binary.memory_request_mib")]
    pub loki_single_binary_memory_request_mib: Option<u32>,
    #[serde(alias = "loki.single_binary.memory_limit_mib")]
    pub loki_single_binary_memory_limit_mib: Option<u32>,
    #[serde(alias = "loki.write.cpu_request_m")]
    pub loki_write_cpu_request_m: Option<u32>,
    #[serde(alias = "loki.write.cpu_limit_m")]
    pub loki_write_cpu_limit_m: Option<u32>,
    #[serde(alias = "loki.write.memory_request_mib")]
    pub loki_write_memory_request_mib: Option<u32>,
    #[serde(alias = "loki.write.memory_limit_mib")]
    pub loki_write_memory_limit_mib: Option<u32>,
    #[serde(alias = "loki.read.cpu_request_m")]
    pub loki_read_cpu_request_m: Option<u32>,
    #[serde(alias = "loki.read.cpu_limit_m")]
    pub loki_read_cpu_limit_m: Option<u32>,
    #[serde(alias = "loki.read.memory_request_mib")]
    pub loki_read_memory_request_mib: Option<u32>,
    #[serde(alias = "loki.read.memory_limit_mib")]
    pub loki_read_memory_limit_mib: Option<u32>,
    #[serde(alias = "loki.backend.cpu_request_m")]
    pub loki_backend_cpu_request_m: Option<u32>,
    #[serde(alias = "loki.backend.cpu_limit_m")]
    pub loki_backend_cpu_limit_m: Option<u32>,
    #[serde(alias = "loki.backend.memory_request_mib")]
    pub loki_backend_memory_request_mib: Option<u32>,
    #[serde(alias = "loki.backend.memory_limit_mib")]
    pub loki_backend_memory_limit_mib: Option<u32>,
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
    #[serde(alias = "aws.eks.ec2.ami", default = "default_aws_eks_ec2_ami")]
    pub aws_eks_ec2_ami: Ec2Ami,
    #[serde(alias = "aws.vpc.enable_s3_flow_logs")]
    pub aws_vpc_enable_flow_logs: bool,
    #[serde(alias = "aws.vpc.flow_logs_retention_days")]
    pub aws_vpc_flow_logs_retention_days: u32,
    #[serde(alias = "aws.vpc.enable_nat_gateway_secondary_eip")]
    pub aws_vpc_enable_nat_gateway_secondary_eip: bool,
    #[serde(alias = "aws.ecr.enable_pull_through_cache")]
    pub aws_ecr_enable_pull_through_cache: bool,
    #[serde(alias = "aws.eks.enable_alb_controller")]
    pub aws_eks_enable_alb_controller: bool,
    #[serde(
        alias = "aws.eks.alb_controller.replicas",
        default = "default_aws_alb_controller_replicas"
    )]
    pub aws_eks_alb_controller_replicas: u32,
    #[serde(alias = "aws.eks.alb_controller.vpa.vcpu.min_in_milli_cpu")]
    pub aws_eks_alb_controller_vpa_min_vcpu_in_milli_cpu: u32,
    #[serde(alias = "aws.eks.alb_controller.vpa.vcpu.max_in_milli_cpu")]
    pub aws_eks_alb_controller_vpa_max_vcpu_in_milli_cpu: u32,
    #[serde(alias = "aws.eks.alb_controller.vpa.memory.min_in_mib")]
    pub aws_eks_alb_controller_vpa_min_memory_in_mib: u32,
    #[serde(alias = "aws.eks.alb_controller.vpa.memory.max_in_mib")]
    pub aws_eks_alb_controller_vpa_max_memory_in_mib: u32,
    #[serde(alias = "aws.eks.alb_controller.load_balancer_scheme", default)]
    pub aws_eks_alb_controller_load_balancer_scheme: AwsAlbLoadBalancerScheme,
    #[serde(alias = "aws.cloudwatch.eks_logs_retention_days")]
    pub aws_cloudwatch_eks_logs_retention_days: u32,
    #[serde(alias = "aws.eks.encrypt_secrets_kms_key_arn", default)]
    pub aws_eks_encrypt_secrets_kms_key_arn: String,
    #[serde(alias = "aws.eks.enable_pod_identity_addon")]
    pub aws_eks_enable_pod_identity_addon: bool,
    #[serde(alias = "aws.eks.enable_efs_addon")]
    pub aws_eks_enable_efs_addon: bool,
    #[serde(alias = "aws.eks.efs.throughput_mode")]
    pub aws_eks_efs_throughput_mode: EfsThroughputMode,
    #[serde(alias = "aws.eks.efs.performance_mode")]
    pub aws_eks_efs_performance_mode: EfsPerformanceMode,
    #[serde(alias = "aws.eks.efs.transition_to_ia")]
    pub aws_eks_efs_transition_to_ia: EfsTransitionToIa,
    #[serde(alias = "cloud_provider.container_registry.tags")]
    pub cloud_provider_container_registry_tags: HashMap<String, String>,
    #[serde(alias = "database.postgresql.deny_any_access")]
    pub database_postgresql_deny_any_access: bool,
    #[serde(alias = "database.postgresql.allowed_cidrs")]
    pub database_postgresql_allowed_cidrs: Vec<String>,
    #[serde(alias = "database.mysql.deny_any_access")]
    pub database_mysql_deny_any_access: bool,
    #[serde(alias = "database.mysql.allowed_cidrs")]
    pub database_mysql_allowed_cidrs: Vec<String>,
    #[serde(alias = "database.redis.deny_any_access")]
    pub database_redis_deny_any_access: bool,
    #[serde(alias = "database.redis.allowed_cidrs")]
    pub database_redis_allowed_cidrs: Vec<String>,
    #[serde(alias = "database.mongodb.deny_any_access")]
    pub database_mongodb_deny_any_access: bool,
    #[serde(alias = "database.mongodb.allowed_cidrs")]
    pub database_mongodb_allowed_cidrs: Vec<String>,
    #[serde(alias = "dns.coredns.extra_config")]
    pub dns_coredns_extra_config: Option<String>,
    #[serde(alias = "registry.mirroring_mode", default = "default_registry_mirroring_mode")]
    pub registry_mirroring_mode: RegistryMirroringMode,
    #[serde(alias = "nginx.vcpu.request_in_milli_cpu")]
    pub nginx_vcpu_request_in_milli_cpu: u32, // TODO(benjaminch QOV-1400): to be removed
    #[serde(alias = "nginx.vcpu.limit_in_milli_cpu")]
    pub nginx_vcpu_limit_in_milli_cpu: u32, // TODO(benjaminch QOV-1400): to be removed
    #[serde(alias = "nginx.memory.request_in_mib")]
    pub nginx_memory_request_in_mib: u32, // TODO(benjaminch QOV-1400): to be removed
    #[serde(alias = "nginx.memory.limit_in_mib")]
    pub nginx_memory_limit_in_mib: u32, // TODO(benjaminch QOV-1400): to be removed
    #[serde(alias = "nginx.hpa.cpu_utilization_percentage_threshold")]
    pub nginx_hpa_cpu_utilization_percentage_threshold: u32, // TODO(benjaminch QOV-1400): deprecated, to be removed once envoy is default
    #[serde(alias = "nginx.hpa.min_number_instances")]
    pub nginx_hpa_min_number_instances: u32, // TODO(benjaminch QOV-1400): deprecated, to be removed once envoy is default
    #[serde(alias = "nginx.controller.enable_client_ip")]
    pub nginx_controller_enable_client_ip: bool, // TODO(benjaminch QOV-1400): deprecated, to be removed once envoy is default
    #[serde(alias = "nginx.controller.use_forwarded_headers")]
    pub nginx_controller_use_forwarded_headers: bool, // TODO(benjaminch QOV-1400): deprecated, to be removed once envoy is default
    #[serde(alias = "nginx.controller.compute_full_forwarded_for")]
    pub nginx_controller_compute_full_forwarded_for: bool, // TODO(benjaminch QOV-1400): deprecated, to be removed once envoy is default
    #[serde(alias = "nginx.controller.log_format_upstream")]
    pub nginx_controller_log_format_upstream: Option<String>, // TODO(benjaminch QOV-1400): deprecated, to be removed once envoy is default
    #[serde(
        alias = "nginx.controller.log_format_escaping",
        default = "default_nginx_controller_log_format_escaping"
    )]
    pub nginx_controller_log_format_escaping: LogFormatEscaping,
    #[serde(alias = "nginx.controller.http_snippet")]
    pub nginx_controller_http_snippet: Option<NginxHttpSnippet>,
    #[serde(alias = "nginx.controller.server_snippet")]
    pub nginx_controller_server_snippet: Option<NginxServerSnippet>,
    #[serde(alias = "nginx.controller.limit_request_status_code")]
    pub nginx_controller_limit_request_status_code: Option<NginxLimitRequestStatusCode>,
    #[serde(alias = "nginx.controller.configuration_snippet")]
    pub nginx_controller_configuration_snippet: Option<NginxConfigurationSnippet>,
    #[serde(alias = "nginx.hpa.max_number_instances")]
    pub nginx_hpa_max_number_instances: u32, // TODO(benjaminch QOV-1400): deprecated, to be removed once envoy is default

    #[serde(alias = "nginx.controller.custom_http_errors")]
    pub nginx_controller_custom_http_errors: Option<String>, // TODO(benjaminch QOV-1400): deprecated, to be removed once envoy is default
    #[serde(alias = "nginx.controller.enable_compression")]
    pub nginx_controller_enable_compression: bool, // TODO(benjaminch QOV-1400): deprecated, to be removed once envoy is default
    #[serde(alias = "nginx.default_backend.enabled")]
    pub nginx_default_backend_enabled: Option<bool>, // TODO(benjaminch QOV-1400): deprecated, to be removed once envoy is default
    #[serde(alias = "nginx.default_backend.image_repository")]
    pub nginx_default_backend_image_repository: Option<String>, // TODO(benjaminch QOV-1400): deprecated, to be removed once envoy is default
    #[serde(alias = "nginx.default_backend.image_tag")]
    pub nginx_default_backend_image_tag: Option<String>, // TODO(benjaminch QOV-1400): deprecated, to be removed once envoy is default

    #[serde(alias = "scaleway.enable_private_network_migration")]
    pub scaleway_enable_private_network_migration: bool,
    #[serde(alias = "gcp.vpc.enable_flow_logs")]
    pub gcp_vpc_enable_flow_logs: bool,
    #[serde(alias = "gcp.vpc.flow_logs_sampling")]
    pub gcp_vpc_flow_logs_sampling: Option<Percentage>,
    #[serde(alias = "qovery.static_ip_mode")]
    pub qovery_static_ip_mode: Option<bool>,
    #[serde(alias = "k8s.api.allowed_public_access_cidrs")]
    pub k8s_api_allowed_public_access_cidrs: Option<Vec<String>>,
    #[serde(alias = "storageclass.fast_ssd")]
    pub k8s_storage_class_fast_ssd: StorageClass,

    #[serde(alias = "object_storage.enable_logging")]
    pub object_storage_enable_logging: bool,

    #[serde(alias = "aws.metrics_server.replicas")]
    pub aws_metrics_server_replicas: Option<u32>,

    #[serde(alias = "k8s.deploy_api_gateway", default)]
    pub k8s_deploy_api_gateway: Option<bool>,
    #[serde(alias = "k8s.use_api_gateway", default)]
    pub k8s_use_api_gateway: Option<bool>,
    #[serde(alias = "k8s.remove_nginx", default)]
    pub k8s_remove_nginx: Option<bool>,

    #[serde(alias = "envoy.hpa.cpu_average_utilization_percentage_threshold")]
    pub envoy_hpa_cpu_average_utilization_percentage_threshold: Option<Percentage>,
    #[serde(alias = "envoy.hpa.memory_average_utilization_percentage_threshold")]
    pub envoy_hpa_memory_average_utilization_percentage_threshold: Option<Percentage>,
    #[serde(alias = "envoy.hpa.min_number_instances")]
    pub envoy_hpa_min_number_instances: u32,
    #[serde(alias = "envoy.hpa.max_number_instances")]
    pub envoy_hpa_max_number_instances: u32,
    #[serde(
        alias = "envoy.gateway_controller.replicas",
        default = "default_envoy_gateway_controller_replicas"
    )]
    pub envoy_gateway_controller_replicas: u32,
    #[serde(alias = "envoy.vcpu.request_in_milli_cpu")]
    pub envoy_vcpu_request_in_milli_cpu: u32,
    #[serde(alias = "envoy.vcpu.limit_in_milli_cpu")]
    pub envoy_vcpu_limit_in_milli_cpu: u32,
    #[serde(alias = "envoy.memory.request_in_mib")]
    pub envoy_memory_request_in_mib: u32,
    #[serde(alias = "envoy.memory.limit_in_mib")]
    pub envoy_memory_limit_in_mib: u32,
    #[serde(alias = "envoy.gateway_api.http_request_timeout_seconds")]
    pub envoy_gateway_api_http_request_timeout_seconds: Option<u32>,
    #[serde(alias = "envoy.gateway_api.http_connection_idle_timeout_seconds")]
    pub envoy_gateway_api_http_connection_idle_timeout_seconds: Option<u32>,
    #[serde(alias = "envoy.gateway_api.http_stream_idle_timeout_seconds")]
    pub envoy_gateway_api_http_stream_idle_timeout_seconds: Option<u32>,
    #[serde(alias = "envoy.gateway_api.http_max_stream_duration_seconds")]
    pub envoy_gateway_api_http_max_stream_duration_seconds: Option<u32>,
    #[serde(alias = "envoy.gateway_api.retry.num_retries")]
    pub envoy_gateway_api_retry_num_retries: Option<u32>,
    #[serde(
        alias = "envoy.gateway_api.retry.retry_on",
        with = "crate::io_models::types::gateway_api_retry_triggers"
    )]
    pub envoy_gateway_api_retry_retry_on: Option<Vec<GatewayApiRetryTrigger>>,
    #[serde(
        alias = "envoy.gateway_api.retry.http_status_codes",
        with = "crate::io_models::types::http_status_codes"
    )]
    pub envoy_gateway_api_retry_http_status_codes: Option<Vec<u16>>,
    #[serde(alias = "envoy.gateway_api.retry.per_try_timeout_seconds")]
    pub envoy_gateway_api_retry_per_try_timeout_seconds: Option<u32>,
    #[serde(alias = "envoy.gateway_api.path.disable_merge_slashes", default)]
    pub envoy_gateway_api_path_disable_merge_slashes: bool,
    #[serde(
        alias = "envoy.gateway_api.path.escaped_slashes_action",
        default = "default_envoy_gateway_api_path_escaped_slashes_action"
    )]
    pub envoy_gateway_api_path_escaped_slashes_action: EnvoyGatewayApiPathEscapedSlashesAction,
    #[serde(alias = "envoy.client_ip_detection.x_forwarded_for.number_trusted_hops")]
    pub envoy_client_ip_detection_x_forwarded_for_number_trusted_hops: Option<u8>,
    #[serde(alias = "envoy.client_ip_detection.x_forwarded_for.trusted_cidrs", default)]
    pub envoy_client_ip_detection_x_forwarded_for_trusted_cidrs: Vec<IpNet>,
    #[serde(alias = "envoy.client_validation.ca_certificates", default)]
    pub envoy_client_validation_ca_certificates: Vec<EnvoyClientValidationCaCertificate>,
    #[serde(alias = "envoy.access_log.format", alias = "envoy.log_format")]
    pub envoy_access_log_format: Option<String>,
    #[serde(
        alias = "envoy.custom_http_errors.default",
        serialize_with = "crate::io_models::types::http_status_codes::serialize",
        deserialize_with = "crate::io_models::types::http_status_codes::deserialize"
    )]
    pub envoy_custom_http_errors_default: Option<Vec<u16>>,
    #[serde(alias = "envoy.enable_compression")]
    pub envoy_enable_compression: bool,
    #[serde(alias = "envoy.default_backend.enable")]
    pub envoy_default_backend_enable: bool,
    #[serde(alias = "envoy.default_backend.image")]
    pub envoy_default_backend_image: Option<String>,
    #[serde(alias = "envoy.default_backend.tag")]
    pub envoy_default_backend_tag: Option<String>,
    #[serde(alias = "envoy.custom_certificate")]
    pub envoy_custom_certificate: Option<Certificate>,
}

impl Default for ClusterAdvancedSettings {
    fn default() -> Self {
        let default_database_cirds = vec!["0.0.0.0/0".to_string()];

        ClusterAdvancedSettings {
            cluster_profile: ClusterProfile::Medium,
            load_balancer_size: "lb-s".to_string(),
            load_balancer_source_ranges: vec![],
            load_balancer_ip_allocation_ids: None,
            registry_image_retention_time_sec: 31536000,
            pleco_resources_ttl: -1,
            loki_log_retention_in_week: 12,
            loki_deployment_mode: LokiDeploymentMode::default(),
            loki_single_binary_cpu_request_m: None,
            loki_single_binary_cpu_limit_m: None,
            loki_single_binary_memory_request_mib: None,
            loki_single_binary_memory_limit_mib: None,
            loki_write_cpu_request_m: None,
            loki_write_cpu_limit_m: None,
            loki_write_memory_request_mib: None,
            loki_write_memory_limit_mib: None,
            loki_read_cpu_request_m: None,
            loki_read_cpu_limit_m: None,
            loki_read_memory_request_mib: None,
            loki_read_memory_limit_mib: None,
            loki_backend_cpu_request_m: None,
            loki_backend_cpu_limit_m: None,
            loki_backend_memory_request_mib: None,
            loki_backend_memory_limit_mib: None,
            aws_iam_user_mapper_group_enabled: true,
            aws_iam_user_mapper_group_name: Some("Admins".to_string()),
            aws_iam_user_mapper_sso_enabled: false,
            aws_iam_user_mapper_sso_role_arn: None,
            cloud_provider_container_registry_tags: HashMap::new(),
            aws_eks_ec2_metadata_imds: AwsEc2MetadataImds::Optional,
            aws_eks_ec2_ami: Ec2Ami::AmazonLinux2023,
            aws_vpc_enable_flow_logs: false,
            aws_vpc_flow_logs_retention_days: 365,
            aws_vpc_enable_nat_gateway_secondary_eip: false,
            aws_ecr_enable_pull_through_cache: false,
            aws_eks_enable_alb_controller: false,
            aws_cloudwatch_eks_logs_retention_days: 90,
            database_postgresql_deny_any_access: false,
            database_postgresql_allowed_cidrs: default_database_cirds.clone(),
            database_mysql_deny_any_access: false,
            database_mysql_allowed_cidrs: default_database_cirds.clone(),
            database_redis_deny_any_access: false,
            database_redis_allowed_cidrs: default_database_cirds.clone(),
            database_mongodb_deny_any_access: false,
            database_mongodb_allowed_cidrs: default_database_cirds,
            dns_coredns_extra_config: None,
            registry_mirroring_mode: RegistryMirroringMode::Service,
            nginx_vcpu_request_in_milli_cpu: 100,
            nginx_vcpu_limit_in_milli_cpu: 500,
            nginx_memory_request_in_mib: 768,
            nginx_memory_limit_in_mib: 768,
            nginx_hpa_cpu_utilization_percentage_threshold: 50,
            nginx_hpa_min_number_instances: 2,
            nginx_hpa_max_number_instances: 25,
            nginx_controller_enable_client_ip: false,
            nginx_controller_use_forwarded_headers: false,
            nginx_controller_compute_full_forwarded_for: false,
            nginx_controller_log_format_upstream: None,
            nginx_controller_log_format_escaping: LogFormatEscaping::Default,
            nginx_controller_http_snippet: None,
            nginx_controller_server_snippet: None,
            nginx_controller_configuration_snippet: None,
            nginx_controller_limit_request_status_code: None,
            scaleway_enable_private_network_migration: false,
            aws_eks_encrypt_secrets_kms_key_arn: "".to_string(),
            aws_eks_enable_pod_identity_addon: false,
            aws_eks_enable_efs_addon: false,
            aws_eks_efs_throughput_mode: EfsThroughputMode::Elastic,
            aws_eks_efs_performance_mode: EfsPerformanceMode::GeneralPurpose,
            aws_eks_efs_transition_to_ia: EfsTransitionToIa::After30Days,
            gcp_vpc_enable_flow_logs: false,
            gcp_vpc_flow_logs_sampling: None,
            qovery_static_ip_mode: None,
            k8s_api_allowed_public_access_cidrs: None,
            aws_eks_alb_controller_replicas: 1u32,
            aws_eks_alb_controller_vpa_min_vcpu_in_milli_cpu: 128,
            aws_eks_alb_controller_vpa_max_vcpu_in_milli_cpu: 1000,
            aws_eks_alb_controller_vpa_min_memory_in_mib: 128,
            aws_eks_alb_controller_vpa_max_memory_in_mib: 2000,
            aws_eks_alb_controller_load_balancer_scheme: AwsAlbLoadBalancerScheme::InternetFacing,
            k8s_storage_class_fast_ssd: StorageClass("".to_string()),
            nginx_controller_custom_http_errors: None,
            nginx_controller_enable_compression: true,
            nginx_default_backend_enabled: None,
            nginx_default_backend_image_repository: None,
            nginx_default_backend_image_tag: None,
            object_storage_enable_logging: false,
            aws_metrics_server_replicas: None,
            k8s_use_api_gateway: None,
            k8s_deploy_api_gateway: None,
            k8s_remove_nginx: None,
            envoy_hpa_cpu_average_utilization_percentage_threshold: None,
            envoy_hpa_memory_average_utilization_percentage_threshold: None,
            envoy_hpa_min_number_instances: 2,
            envoy_hpa_max_number_instances: 25,
            envoy_gateway_controller_replicas: 2,
            envoy_vcpu_request_in_milli_cpu: 100,
            envoy_vcpu_limit_in_milli_cpu: 1000,
            envoy_memory_request_in_mib: 256,
            envoy_memory_limit_in_mib: 1024,
            envoy_gateway_api_http_request_timeout_seconds: None,
            envoy_gateway_api_http_connection_idle_timeout_seconds: None,
            envoy_gateway_api_http_stream_idle_timeout_seconds: None,
            envoy_gateway_api_http_max_stream_duration_seconds: None,
            envoy_gateway_api_retry_num_retries: None,
            envoy_gateway_api_retry_retry_on: None,
            envoy_gateway_api_retry_http_status_codes: None,
            envoy_gateway_api_retry_per_try_timeout_seconds: None,
            envoy_gateway_api_path_disable_merge_slashes: false,
            envoy_gateway_api_path_escaped_slashes_action: EnvoyGatewayApiPathEscapedSlashesAction::UnescapeAndRedirect,
            envoy_client_ip_detection_x_forwarded_for_number_trusted_hops: None,
            envoy_client_ip_detection_x_forwarded_for_trusted_cidrs: vec![],
            envoy_client_validation_ca_certificates: vec![],
            envoy_access_log_format: None,
            envoy_custom_http_errors_default: None,
            envoy_enable_compression: true,
            envoy_default_backend_enable: false,
            envoy_default_backend_image: None,
            envoy_default_backend_tag: None,
            envoy_custom_certificate: None,
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

        if let Err(err) = self.to_envoy_client_validation_ca_certificates() {
            return Err(Box::new(EngineError::new_invalid_engine_payload_invalid_field_value(
                event_details,
                err,
            )));
        }

        if self.envoy_gateway_controller_replicas == 0 {
            return Err(Box::new(EngineError::new_invalid_engine_payload_invalid_field_value(
                event_details,
                InputError::InvalidInputFieldValue {
                    field_name: "envoy.gateway_controller.replicas".to_string(),
                    message: "must be greater than 0".to_string(),
                },
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

    pub fn to_envoy_client_validation_ca_certificates(
        &self,
    ) -> Result<Vec<EnvoyClientValidationCaCertificateModel>, InputError> {
        if self.envoy_client_validation_ca_certificates.len() > ENVOY_CLIENT_VALIDATION_MAX_CA_CERTIFICATES {
            return Err(InputError::InvalidInputFieldValue {
                field_name: "envoy.client_validation.ca_certificates".to_string(),
                message: format!(
                    "at most {ENVOY_CLIENT_VALIDATION_MAX_CA_CERTIFICATES} client validation CA certificates are supported"
                ),
            });
        }

        let mut unique_input_names = HashSet::new();
        for certificate in &self.envoy_client_validation_ca_certificates {
            let trimmed_name = certificate.name.trim();
            if !trimmed_name.is_empty() && !unique_input_names.insert(trimmed_name.to_string()) {
                return Err(InputError::InvalidInputFieldValue {
                    field_name: "envoy.client_validation.ca_certificates".to_string(),
                    message: format!("duplicate certificate name `{trimmed_name}`"),
                });
            }
        }

        let certificates: Vec<_> = self
            .envoy_client_validation_ca_certificates
            .iter()
            .map(EnvoyClientValidationCaCertificate::to_model)
            .collect::<Result<_, _>>()?;

        Ok(certificates)
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
    use ipnet::IpNet;
    use uuid::Uuid;

    use crate::helm::HelmChartNamespaces;
    use crate::infrastructure::helm_charts::qovery_cluster_gateway_chart::EnvoyClientValidationCaCertificate as EnvoyClientValidationCaCertificateModel;
    use crate::infrastructure::models::cloud_provider::io::{
        ClusterAdvancedSettings, EnvoyClientValidationCaCertificate, EnvoyGatewayApiPathEscapedSlashesAction,
        InputError, LogFormatEscaping, RegistryMirroringMode, validate_aws_cloudwatch_eks_logs_retention_days,
    };
    use crate::io_models::types::gateway_api_retry_triggers::GatewayApiRetryTrigger;
    use crate::{
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
    fn test_default_cluster_profile() {
        // Test that cluster_profile defaults to Medium when not specified
        let settings = ClusterAdvancedSettings::default();
        assert_eq!(settings.cluster_profile, super::ClusterProfile::Medium);

        // Test that cluster_profile defaults to Medium when deserializing empty JSON
        let data = r#"{}"#;
        let settings: ClusterAdvancedSettings = serde_json::from_str(data).unwrap();
        assert_eq!(settings.cluster_profile, super::ClusterProfile::Medium);
    }

    #[test]
    fn test_cluster_profile_can_be_overridden() {
        // Test that cluster_profile can be overridden via deserialization
        let test_cases = vec![
            (r#"{"cluster.profile": "Small"}"#, super::ClusterProfile::Small),
            (r#"{"cluster.profile": "Medium"}"#, super::ClusterProfile::Medium),
            (r#"{"cluster.profile": "Large"}"#, super::ClusterProfile::Large),
            (r#"{"cluster.profile": "ExtraLarge"}"#, super::ClusterProfile::ExtraLarge),
        ];

        for (json, expected_profile) in test_cases {
            let settings: ClusterAdvancedSettings = serde_json::from_str(json).unwrap();
            assert_eq!(settings.cluster_profile, expected_profile);
        }
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
    fn test_aws_ecr_pull_through_cache_setting_deserialization() {
        let default_settings: ClusterAdvancedSettings = serde_json::from_str("{}").unwrap();
        assert!(!default_settings.aws_ecr_enable_pull_through_cache);

        let enabled_settings: ClusterAdvancedSettings =
            serde_json::from_str(r#"{"aws.ecr.enable_pull_through_cache": true}"#).unwrap();
        assert!(enabled_settings.aws_ecr_enable_pull_through_cache);
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
        assert!(!cluster_advanced_settings.nginx_controller_enable_client_ip);
        assert_eq!(cluster_advanced_settings.nginx_controller_log_format_upstream, None);
        assert_eq!(
            cluster_advanced_settings.nginx_controller_log_format_escaping,
            LogFormatEscaping::Default
        );
        assert!(cluster_advanced_settings.nginx_controller_enable_compression);
    }

    #[test]
    fn test_nginx_deserialization() {
        let nginx_vcpu_request_in_milli_cpu = 155;
        let nginx_hpa_cpu_utilization_percentage_threshold = 75;
        let data = format!(
            r#"
        {{
            "nginx.vcpu.request_in_milli_cpu": {nginx_vcpu_request_in_milli_cpu},
            "nginx.hpa.cpu_utilization_percentage_threshold": {nginx_hpa_cpu_utilization_percentage_threshold},
            "nginx.controller.enable_compression": false
        }}"#
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
        assert!(!cluster_advanced_settings.nginx_controller_enable_compression);
    }

    #[test]
    fn test_nginx_server_snippet_to_model() {
        // setup:
        let snippet_json = r#"{"test": "coucou"}"#;
        let nginx_server_snippet_io = super::NginxServerSnippet(snippet_json.to_string());

        // execute:
        let model = nginx_server_snippet_io.to_model();

        // verify:
        assert_eq!(snippet_json, model.get_snippet_value());
    }

    #[test]
    fn test_nginx_http_snippet_to_model() {
        // setup:
        let snippet_json = r#"{"test": "coucou"}"#;
        let nginx_http_snippet_io = super::NginxHttpSnippet(snippet_json.to_string());

        // execute:
        let model = nginx_http_snippet_io.to_model();

        // verify:
        assert_eq!(snippet_json, model.get_snippet_value());
    }

    #[test]
    fn test_nginx_configuration_snippet_to_model() {
        // setup:
        let snippet_json = r#"{"test": "coucou"}"#;
        let nginx_configuration_snippet_io = super::NginxConfigurationSnippet(snippet_json.to_string());

        // execute:
        let model = nginx_configuration_snippet_io.to_model();

        // verify:
        assert_eq!(snippet_json, model.get_snippet_value());
    }

    #[test]
    fn test_nginx_limit_request_status_code_to_model() {
        // setup:
        let status_code = 200;
        let nginx_limit_request_status_code_io = super::NginxLimitRequestStatusCode(status_code);

        // execute:
        let res = nginx_limit_request_status_code_io
            .to_model()
            .expect("Should be a proper HTTP status code");

        // verify:
        assert_eq!(status_code, res.as_u16());
    }

    #[test]
    fn test_nginx_limit_request_status_code_to_model_wrong_http_code_value() {
        // setup:
        let status_code = 2000;
        let nginx_limit_request_status_code_io = super::NginxLimitRequestStatusCode(status_code);

        // execute:
        let res = nginx_limit_request_status_code_io.to_model();

        // verify:
        assert_eq!(
            InputError::InvalidInputFieldValue {
                field_name: "nginx.controller.limit_request_status_code".to_string(),
                message: "Should be a proper HTTP status code".to_string(),
            },
            res.err().expect("Should be an error")
        );
    }

    #[test]
    fn test_ec2_ami_serde_standard_variants() {
        use super::Ec2Ami;

        let test_cases = vec![
            (Ec2Ami::AmazonLinux2, "\"AmazonLinux2\""),
            (Ec2Ami::AmazonLinux2023, "\"AmazonLinux2023\""),
            (Ec2Ami::Bottlerocket, "\"Bottlerocket\""),
        ];

        for (variant, expected_json) in test_cases {
            // Serialize
            let serialized = serde_json::to_string(&variant).unwrap();
            assert_eq!(serialized, expected_json);

            // Deserialize back
            let deserialized: Ec2Ami = serde_json::from_str(&serialized).unwrap();
            assert_eq!(deserialized, variant);
        }
    }

    #[test]
    fn test_ec2_ami_serde_custom_ami_id() {
        use super::Ec2Ami;

        let ami = Ec2Ami::Custom("ami-0123456789abcdef0".to_string());
        let serialized = serde_json::to_string(&ami).unwrap();
        assert_eq!(serialized, "\"ami-0123456789abcdef0\"");

        let deserialized: Ec2Ami = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, ami);
    }

    #[test]
    fn test_ec2_ami_serde_custom_ami_name_pattern() {
        use super::Ec2Ami;

        let ami = Ec2Ami::Custom("my-custom-ami-*".to_string());
        let serialized = serde_json::to_string(&ami).unwrap();
        assert_eq!(serialized, "\"my-custom-ami-*\"");

        let deserialized: Ec2Ami = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, ami);
    }

    #[test]
    fn test_ec2_ami_deserialize_unknown_string_becomes_custom() {
        use super::Ec2Ami;

        let deserialized: Ec2Ami = serde_json::from_str("\"my-hardened-ami-v2\"").unwrap();
        assert_eq!(deserialized, Ec2Ami::Custom("my-hardened-ami-v2".to_string()));
    }

    #[test]
    fn test_ec2_ami_deserialize_empty_string_fails() {
        use super::Ec2Ami;

        let result = serde_json::from_str::<Ec2Ami>("\"\"");
        assert!(result.is_err());
    }

    #[test]
    fn test_ec2_ami_in_cluster_advanced_settings() {
        // Test custom AMI ID via advanced settings JSON
        let data = r#"{"aws.eks.ec2.ami": "ami-0123456789abcdef0"}"#;
        let settings: ClusterAdvancedSettings = serde_json::from_str(data).unwrap();
        assert_eq!(
            settings.aws_eks_ec2_ami,
            super::Ec2Ami::Custom("ami-0123456789abcdef0".to_string())
        );

        // Test custom AMI name pattern via advanced settings JSON
        let data = r#"{"aws.eks.ec2.ami": "my-custom-ami-*"}"#;
        let settings: ClusterAdvancedSettings = serde_json::from_str(data).unwrap();
        assert_eq!(settings.aws_eks_ec2_ami, super::Ec2Ami::Custom("my-custom-ami-*".to_string()));

        // Test standard variant still works
        let data = r#"{"aws.eks.ec2.ami": "AmazonLinux2023"}"#;
        let settings: ClusterAdvancedSettings = serde_json::from_str(data).unwrap();
        assert_eq!(settings.aws_eks_ec2_ami, super::Ec2Ami::AmazonLinux2023);
    }

    #[test]
    fn test_default_envoy_gateway_api_http_timeouts_are_none() {
        let settings = ClusterAdvancedSettings::default();
        assert_eq!(settings.envoy_gateway_api_http_request_timeout_seconds, None);
        assert_eq!(settings.envoy_gateway_api_http_connection_idle_timeout_seconds, None);
        assert_eq!(settings.envoy_gateway_api_http_stream_idle_timeout_seconds, None);
        assert_eq!(settings.envoy_gateway_api_http_max_stream_duration_seconds, None);
        assert_eq!(settings.envoy_gateway_api_retry_num_retries, None);
        assert_eq!(settings.envoy_gateway_api_retry_retry_on, None);
        assert_eq!(settings.envoy_gateway_api_retry_http_status_codes, None);
        assert_eq!(settings.envoy_gateway_api_retry_per_try_timeout_seconds, None);
        assert!(!settings.envoy_gateway_api_path_disable_merge_slashes);
        assert!(settings.envoy_client_validation_ca_certificates.is_empty());
        assert_eq!(
            settings.envoy_gateway_api_path_escaped_slashes_action,
            EnvoyGatewayApiPathEscapedSlashesAction::UnescapeAndRedirect
        );
    }

    #[test]
    fn test_envoy_gateway_api_http_timeouts_deserialization() {
        let data = r#"
        {
            "envoy.gateway_controller.replicas": 3,
            "envoy.gateway_api.http_request_timeout_seconds": 90,
            "envoy.gateway_api.http_connection_idle_timeout_seconds": 120,
            "envoy.gateway_api.http_stream_idle_timeout_seconds": 300,
            "envoy.gateway_api.http_max_stream_duration_seconds": 600,
            "envoy.gateway_api.retry.num_retries": 2,
            "envoy.gateway_api.retry.retry_on": "connect-failure,reset",
            "envoy.gateway_api.retry.http_status_codes": "503",
            "envoy.gateway_api.retry.per_try_timeout_seconds": 2,
            "envoy.gateway_api.path.disable_merge_slashes": true,
            "envoy.gateway_api.path.escaped_slashes_action": "KeepUnchanged",
            "envoy.client_validation.ca_certificates": [
                {
                    "name": "cloudflare-origin-pull-ca",
                    "ca_crt": "-----BEGIN CERTIFICATE-----\nTEST\n-----END CERTIFICATE-----"
                }
            ]
        }
        "#;
        let settings: ClusterAdvancedSettings = serde_json::from_str(data).unwrap();
        assert_eq!(settings.envoy_gateway_controller_replicas, 3);
        assert_eq!(settings.envoy_gateway_api_http_request_timeout_seconds, Some(90));
        assert_eq!(settings.envoy_gateway_api_http_connection_idle_timeout_seconds, Some(120));
        assert_eq!(settings.envoy_gateway_api_http_stream_idle_timeout_seconds, Some(300));
        assert_eq!(settings.envoy_gateway_api_http_max_stream_duration_seconds, Some(600));
        assert_eq!(settings.envoy_gateway_api_retry_num_retries, Some(2));
        assert_eq!(
            settings.envoy_gateway_api_retry_retry_on,
            Some(vec![GatewayApiRetryTrigger::ConnectFailure, GatewayApiRetryTrigger::Reset,])
        );
        assert_eq!(settings.envoy_gateway_api_retry_http_status_codes, Some(vec![503]));
        assert_eq!(settings.envoy_gateway_api_retry_per_try_timeout_seconds, Some(2));
        assert!(settings.envoy_gateway_api_path_disable_merge_slashes);
        assert_eq!(
            settings.envoy_client_validation_ca_certificates,
            vec![EnvoyClientValidationCaCertificate {
                name: "cloudflare-origin-pull-ca".to_string(),
                ca_crt: "-----BEGIN CERTIFICATE-----\nTEST\n-----END CERTIFICATE-----".to_string(),
            }]
        );
        assert_eq!(
            settings.envoy_gateway_api_path_escaped_slashes_action,
            EnvoyGatewayApiPathEscapedSlashesAction::KeepUnchanged
        );
    }

    #[test]
    fn test_envoy_client_validation_ca_certificates_to_model_sets_qovery_namespace() {
        let settings = ClusterAdvancedSettings {
            envoy_client_validation_ca_certificates: vec![
                EnvoyClientValidationCaCertificate {
                    name: "cloudflare.origin-pull-ca".to_string(),
                    ca_crt: "-----BEGIN CERTIFICATE-----\nPRIMARY\n-----END CERTIFICATE-----".to_string(),
                },
                EnvoyClientValidationCaCertificate {
                    name: "shared-origin-pull-ca".to_string(),
                    ca_crt: "-----BEGIN CERTIFICATE-----\nBACKUP\n-----END CERTIFICATE-----".to_string(),
                },
            ],
            ..Default::default()
        };

        let certificates = settings
            .to_envoy_client_validation_ca_certificates()
            .expect("certificates should parse");

        assert_eq!(
            certificates,
            vec![
                EnvoyClientValidationCaCertificateModel {
                    name: "envoy-client-validation-cloudflare.origin-pull-ca".to_string(),
                    namespace: HelmChartNamespaces::Qovery,
                    ca_crt: "-----BEGIN CERTIFICATE-----\nPRIMARY\n-----END CERTIFICATE-----".to_string(),
                },
                EnvoyClientValidationCaCertificateModel {
                    name: "envoy-client-validation-shared-origin-pull-ca".to_string(),
                    namespace: HelmChartNamespaces::Qovery,
                    ca_crt: "-----BEGIN CERTIFICATE-----\nBACKUP\n-----END CERTIFICATE-----".to_string(),
                },
            ]
        );
    }

    #[test]
    fn test_envoy_client_validation_ca_certificates_reject_empty_name() {
        let settings = ClusterAdvancedSettings {
            envoy_client_validation_ca_certificates: vec![EnvoyClientValidationCaCertificate {
                name: "   ".to_string(),
                ca_crt: "-----BEGIN CERTIFICATE-----\nTEST\n-----END CERTIFICATE-----".to_string(),
            }],
            ..Default::default()
        };

        let err = settings
            .to_envoy_client_validation_ca_certificates()
            .expect_err("empty certificate name should be rejected");

        assert_eq!(
            err,
            InputError::InvalidInputFieldValue {
                field_name: "envoy.client_validation.ca_certificates".to_string(),
                message: "certificate `name` must not be empty".to_string(),
            }
        );
    }

    #[test]
    fn test_envoy_client_validation_ca_certificates_reject_empty_content() {
        let settings = ClusterAdvancedSettings {
            envoy_client_validation_ca_certificates: vec![EnvoyClientValidationCaCertificate {
                name: "cloudflare-origin-pull-ca".to_string(),
                ca_crt: " \n\t ".to_string(),
            }],
            ..Default::default()
        };

        let err = settings
            .to_envoy_client_validation_ca_certificates()
            .expect_err("empty certificate content should be rejected");

        assert_eq!(
            err,
            InputError::InvalidInputFieldValue {
                field_name: "envoy.client_validation.ca_certificates".to_string(),
                message: "certificate `cloudflare-origin-pull-ca` has an empty `ca_crt`".to_string(),
            }
        );
    }

    #[test]
    fn test_envoy_client_validation_ca_certificates_reject_invalid_kubernetes_secret_name() {
        let settings = ClusterAdvancedSettings {
            envoy_client_validation_ca_certificates: vec![EnvoyClientValidationCaCertificate {
                name: "Cloudflare_Origin_Pull_CA".to_string(),
                ca_crt: "-----BEGIN CERTIFICATE-----\nTEST\n-----END CERTIFICATE-----".to_string(),
            }],
            ..Default::default()
        };

        let err = settings
            .to_envoy_client_validation_ca_certificates()
            .expect_err("invalid kubernetes secret name should be rejected");

        assert_eq!(
            err,
            InputError::InvalidInputFieldValue {
                field_name: "envoy.client_validation.ca_certificates".to_string(),
                message: "certificate `Cloudflare_Origin_Pull_CA` has an invalid `name`: expected a valid Kubernetes DNS-1123 subdomain".to_string(),
            }
        );
    }

    #[test]
    fn test_envoy_client_validation_ca_certificates_reject_name_when_prefixed_secret_name_exceeds_limit() {
        let settings = ClusterAdvancedSettings {
            envoy_client_validation_ca_certificates: vec![EnvoyClientValidationCaCertificate {
                name: "a".repeat(230),
                ca_crt: "-----BEGIN CERTIFICATE-----\nTEST\n-----END CERTIFICATE-----".to_string(),
            }],
            ..Default::default()
        };

        let err = settings
            .to_envoy_client_validation_ca_certificates()
            .expect_err("final prefixed secret name should respect kubernetes length limit");

        assert_eq!(
            err,
            InputError::InvalidInputFieldValue {
                field_name: "envoy.client_validation.ca_certificates".to_string(),
                message: format!(
                    "certificate `{}` has an invalid `name`: final secret name `envoy-client-validation-{}` must be a valid Kubernetes DNS-1123 subdomain",
                    "a".repeat(230),
                    "a".repeat(230)
                ),
            }
        );
    }

    #[test]
    fn test_envoy_client_validation_ca_certificates_reject_duplicate_names() {
        let settings = ClusterAdvancedSettings {
            envoy_client_validation_ca_certificates: vec![
                EnvoyClientValidationCaCertificate {
                    name: "cloudflare-origin-pull-ca".to_string(),
                    ca_crt: "-----BEGIN CERTIFICATE-----\nONE\n-----END CERTIFICATE-----".to_string(),
                },
                EnvoyClientValidationCaCertificate {
                    name: "cloudflare-origin-pull-ca".to_string(),
                    ca_crt: "-----BEGIN CERTIFICATE-----\nTWO\n-----END CERTIFICATE-----".to_string(),
                },
            ],
            ..Default::default()
        };

        let err = settings
            .to_envoy_client_validation_ca_certificates()
            .expect_err("duplicate secret names should be rejected");

        assert_eq!(
            err,
            InputError::InvalidInputFieldValue {
                field_name: "envoy.client_validation.ca_certificates".to_string(),
                message: "duplicate certificate name `cloudflare-origin-pull-ca`".to_string(),
            }
        );
    }

    #[test]
    fn test_envoy_client_validation_ca_certificates_reject_more_than_crd_limit() {
        let settings = ClusterAdvancedSettings {
            envoy_client_validation_ca_certificates: (0..=super::ENVOY_CLIENT_VALIDATION_MAX_CA_CERTIFICATES)
                .map(|index| EnvoyClientValidationCaCertificate {
                    name: format!("cloudflare-origin-pull-ca-{index}"),
                    ca_crt: format!("-----BEGIN CERTIFICATE-----\nTEST-{index}\n-----END CERTIFICATE-----"),
                })
                .collect(),
            ..Default::default()
        };

        let err = settings
            .to_envoy_client_validation_ca_certificates()
            .expect_err("certificate count above the CRD limit should be rejected");

        assert_eq!(
            err,
            InputError::InvalidInputFieldValue {
                field_name: "envoy.client_validation.ca_certificates".to_string(),
                message: "at most 8 client validation CA certificates are supported".to_string(),
            }
        );
    }

    #[test]
    fn test_envoy_gateway_controller_replicas_deserialization_defaults_when_missing() {
        let data = r#"
        {
            "envoy.gateway_api.http_request_timeout_seconds": 90
        }
        "#;

        let settings: ClusterAdvancedSettings = serde_json::from_str(data).unwrap();

        assert_eq!(settings.envoy_gateway_controller_replicas, 2);
        assert_eq!(settings.envoy_gateway_api_http_request_timeout_seconds, Some(90));
    }

    #[test]
    fn test_envoy_gateway_controller_replicas_validation_rejects_zero() {
        let settings = ClusterAdvancedSettings {
            envoy_gateway_controller_replicas: 0,
            ..Default::default()
        };
        let event_details = EventDetails::new(
            None,
            QoveryIdentifier::default(),
            QoveryIdentifier::default(),
            "".to_string(),
            Stage::Infrastructure(crate::events::InfrastructureStep::ValidateApiInput),
            Transmitter::Kubernetes(Uuid::new_v4(), "".to_string()),
        );

        let result = settings.validate(event_details);

        assert!(result.is_err());
    }

    #[test]
    fn test_default_envoy_client_ip_detection_xff_trusted_cidrs_is_empty() {
        let settings = ClusterAdvancedSettings::default();
        assert!(
            settings
                .envoy_client_ip_detection_x_forwarded_for_trusted_cidrs
                .is_empty()
        );
    }

    #[test]
    fn test_envoy_client_ip_detection_xff_trusted_cidrs_deserialization() {
        let data = r#"
        {
            "envoy.client_ip_detection.x_forwarded_for.trusted_cidrs": [
                "10.0.0.0/8",
                "192.168.0.0/16",
                "::/0"
            ]
        }
        "#;
        let settings: ClusterAdvancedSettings = serde_json::from_str(data).unwrap();
        assert_eq!(
            settings.envoy_client_ip_detection_x_forwarded_for_trusted_cidrs,
            vec![
                IpNet::V4("10.0.0.0/8".parse().unwrap_or_default()),
                IpNet::V4("192.168.0.0/16".parse().unwrap_or_default()),
                IpNet::V6("::/0".parse().unwrap_or_default())
            ]
        );
    }
}

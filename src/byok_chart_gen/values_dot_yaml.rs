use std::collections::BTreeMap;

use derive_more::Display;
use serde_derive::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Display, Ord, PartialOrd, PartialEq, Eq, Debug)]
pub enum ChartCategory {
    #[display("services")]
    Services,
    #[display("qovery")]
    Qovery,
    #[display("ingress")]
    Ingress,
    #[display("dns")]
    Dns,
    #[display("logging")]
    Logging,
    #[display("certificates")]
    Certificates,
    #[display("observability")]
    Observability,
    #[display("aws")]
    Aws,
    #[display("gcp")]
    Gcp,
    #[display("scaleway")]
    Scaleway,
}

#[derive(Serialize, Deserialize)]
pub struct ValuesFile {
    pub services: ServicesEnabler,
    pub qovery: QoveryGlobalConfig,
    #[serde(rename = "qovery-cluster-agent")]
    pub qovery_cluster_agent: QoveryClusterAgent,
    #[serde(rename = "qovery-shell-agent")]
    pub qovery_shell_agent: QoveryShellAgent,
    #[serde(rename = "qovery-engine", default, skip_serializing_if = "Option::is_none")]
    pub qovery_engine: Option<QoveryEngine>,
    #[serde(rename = "ingress-nginx")]
    pub ingress_nginx: ChartConfig,
    #[serde(rename = "external-dns")]
    pub external_dns: ChartConfig,
    pub promtail: ChartConfig,
    pub loki: ChartConfig,
    #[serde(rename = "cert-manager")]
    pub cert_manager: ChartConfig,
    #[serde(rename = "qovery-cert-manager-webhook")]
    pub cert_manager_qovery_webhook: ChartConfig,
    #[serde(rename = "cert-manager-configs")]
    pub cert_manager_configs: ChartConfig,
    #[serde(rename = "q-storageclass-aws", default, skip_serializing_if = "Option::is_none")]
    pub qovery_storage_class_aws: Option<ChartConfig>,
    #[serde(rename = "q-storageclass-gcp", default, skip_serializing_if = "Option::is_none")]
    pub qovery_storage_class_gcp: Option<ChartConfig>,
    #[serde(
        rename = "q-storageclass-scaleway",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub qovery_storage_class_scaleway: Option<ChartConfig>,
    #[serde(rename = "metrics-server", skip_serializing_if = "Option::is_none")]
    pub metrics_server: Option<ChartConfig>,
    #[serde(rename = "aws-load-balancer-controller", skip_serializing_if = "Option::is_none")]
    pub aws_load_balancer_controller: Option<ChartConfig>,
}

#[derive(Serialize, Deserialize)]
pub struct ChartConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub override_chart: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QoveryGlobalConfig {
    #[serde(rename = "clusterId")]
    pub cluster_id: String,
    #[serde(rename = "clusterShortId")]
    pub cluster_short_id: String,
    #[serde(rename = "organizationId")]
    pub organization_id: String,
    #[serde(rename = "jwtToken")]
    pub jwt_token: String,
    #[serde(rename = "rootDomain")]
    pub root_domain: String,
    pub domain: String,
    #[serde(rename = "domainWildcard")]
    pub domain_wildcard: String,
    #[serde(rename = "qoveryDnsUrl")]
    pub qovery_dns_url: String,
    #[serde(rename = "agentGatewayUrl")]
    pub agent_gateway_url: String,
    #[serde(rename = "engineGatewayUrl")]
    pub engine_gateway_url: String,
    #[serde(rename = "lokiUrl")]
    pub loki_url: String,
    #[serde(rename = "promtailLokiUrl")]
    pub promtail_loki_url: String,
    #[serde(rename = "acmeEmailAddr")]
    pub acme_email_addr: String,
    #[serde(rename = "externalDnsPrefix")]
    pub external_dns_prefix: String,
    pub architectures: String,
    #[serde(rename = "engineVersion")]
    pub engine_version: String,
    #[serde(rename = "shellAgentVersion")]
    pub shell_agent_version: String,
    #[serde(rename = "clusterAgentVersion")]
    pub cluster_agent_version: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QoveryShellAgent {
    pub fullname_override: String,
    pub image: ImageTag,
    pub environment_variables: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QoveryClusterAgent {
    pub fullname_override: String,
    pub image: ImageTag,
    pub environment_variables: BTreeMap<String, String>,
    pub use_self_sign_certificate: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QoveryEngine {
    pub image: ImageTag,
    pub engine_resources: Option<EngineResources>,
    pub build_container: BuildContainer,
    pub environment_variables: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineResources {
    pub cpu: String,
    pub memory: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildContainer {
    pub environment_variables: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize)]
pub struct ImageTag {
    pub tag: String,
}

#[derive(Serialize, Deserialize)]
pub struct ServicesEnabler {
    pub qovery: QoveryServices,
    pub ingress: IngressServices,
    pub dns: DnsServices,
    pub logging: LoggingServices,
    pub certificates: CertificateServices,
    pub observability: ObservabilityServices,
    pub aws: AwsServices,
    pub gcp: GcpServices,
    pub scaleway: ScalewayServices,
}

#[derive(Serialize, Deserialize)]
pub struct QoveryServices {
    #[serde(rename = "qovery-cluster-agent")]
    pub qovery_cluster_agent: ServiceEnabled,
    #[serde(rename = "qovery-shell-agent")]
    pub qovery_shell_agent: ServiceEnabled,
    #[serde(rename = "qovery-engine")]
    pub qovery_engine: ServiceEnabled,
    #[serde(rename = "qovery-priority-class")]
    pub priority_class: ServiceEnabled,
}

#[derive(Serialize, Deserialize)]
pub struct IngressServices {
    #[serde(rename = "ingress-nginx")]
    pub ingress_nginx: ServiceEnabled,
}

#[derive(Serialize, Deserialize)]
pub struct DnsServices {
    #[serde(rename = "external-dns")]
    pub external_dns: ServiceEnabled,
}

#[derive(Serialize, Deserialize)]
pub struct LoggingServices {
    pub loki: ServiceEnabled,
    pub promtail: ServiceEnabled,
}

#[derive(Serialize, Deserialize)]
pub struct CertificateServices {
    #[serde(rename = "cert-manager")]
    pub cert_manager: ServiceEnabled,
    #[serde(rename = "cert-manager-configs")]
    pub cert_manager_configs: ServiceEnabled,
    #[serde(rename = "qovery-cert-manager-webhook")]
    pub cert_manager_qovery_webhook: ServiceEnabled,
}

#[derive(Serialize, Deserialize)]
pub struct ObservabilityServices {
    #[serde(rename = "metrics-server", skip_serializing_if = "Option::is_none")]
    pub metrics_server: Option<ServiceEnabled>,
}

#[derive(Serialize, Deserialize)]
pub struct AwsServices {
    #[serde(rename = "q-storageclass-aws")]
    pub qovery_storage_class: ServiceEnabled,
    #[serde(rename = "aws-ebs-csi-driver")]
    pub aws_ebs_csi_driver: ServiceEnabled,
    #[serde(rename = "aws-load-balancer-controller")]
    pub aws_load_balancer_controller: ServiceEnabled,
}

#[derive(Serialize, Deserialize)]
pub struct GcpServices {
    #[serde(rename = "q-storageclass-gcp")]
    pub qovery_storage_class: ServiceEnabled,
}

#[derive(Serialize, Deserialize)]
pub struct ScalewayServices {
    #[serde(rename = "q-storageclass-scaleway")]
    pub qovery_storage_class: ServiceEnabled,
}

#[derive(Serialize, Deserialize)]
pub struct ServiceEnabled {
    pub enabled: bool,
}

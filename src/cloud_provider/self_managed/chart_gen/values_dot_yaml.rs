use std::collections::BTreeMap;

use derive_more::Display;
use serde_derive::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Display, Debug)]
pub enum ChartCategory {
    #[display("services")]
    Services,
    #[display("qovery")]
    Qovery,
    #[display("ingress")]
    Ingress,
    #[display("dns")]
    DNS,
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
    pub qovery_cluster_agent: QoveryAgents,
    #[serde(rename = "qovery-shell-agent")]
    pub qovery_shell_agent: QoveryAgents,
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
    #[serde(rename = "q-storageclass", default, skip_serializing_if = "Option::is_none")]
    pub qovery_storage_class: Option<ChartConfig>,
    #[serde(rename = "metrics-server", skip_serializing_if = "Option::is_none")]
    pub metrics_server: Option<ChartConfig>,
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
    pub domain: String,
    #[serde(rename = "grpcServer")]
    pub grpc_server: String,
    #[serde(rename = "engineGrpcServer")]
    pub engine_grpc_server: String,
    #[serde(rename = "qoveryDnsUrl")]
    pub qovery_dns_url: String,
    #[serde(rename = "lokiUrl")]
    pub loki_url: String,
    #[serde(rename = "promtailLokiUrl")]
    pub promtail_loki_url: String,
    #[serde(rename = "acmeEmailAddr")]
    pub acme_email_addr: String,
    #[serde(rename = "externalDnsPrefix")]
    pub external_dns_prefix: String,
    pub architectures: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QoveryAgents {
    pub full_name_override: String,
    pub image: ImageTag,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aws: Option<AwsServices>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gcp: Option<AwsServices>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scaleway: Option<AwsServices>,
}

#[derive(Serialize, Deserialize)]
pub struct QoveryServices {
    #[serde(rename = "qovery-cluster-agent")]
    pub qovery_cluster_agent: ServiceEnabled,
    #[serde(rename = "qovery-shell-agent")]
    pub qovery_shell_agent: ServiceEnabled,
    #[serde(rename = "qovery-engine")]
    pub qovery_engine: ServiceEnabled,
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
    #[serde(rename = "q-storageclass")]
    pub qovery_storage_class: ServiceEnabled,
}

#[derive(Serialize, Deserialize)]
pub struct GcpServices {
    #[serde(rename = "q-storageclass")]
    pub qovery_storage_class: ServiceEnabled,
}

#[derive(Serialize, Deserialize)]
pub struct ScalewayServices {
    #[serde(rename = "q-storageclass")]
    pub qovery_storage_class: ServiceEnabled,
}

#[derive(Serialize, Deserialize)]
pub struct ServiceEnabled {
    pub enabled: bool,
}

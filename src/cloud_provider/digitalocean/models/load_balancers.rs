#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadBalancer {
    pub id: String,
    pub name: String,
    pub ip: String,
    pub size: String,
    pub algorithm: String,
    pub status: String,
    #[serde(rename = "created_at")]
    pub created_at: String,
    #[serde(rename = "forwarding_rules")]
    pub forwarding_rules: Vec<ForwardingRule>,
    #[serde(rename = "health_check")]
    pub health_check: HealthCheck,
    #[serde(rename = "sticky_sessions")]
    pub sticky_sessions: StickySessions,
    pub region: Region,
    pub tag: String,
    #[serde(rename = "droplet_ids")]
    pub droplet_ids: Vec<i64>,
    #[serde(rename = "redirect_http_to_https")]
    pub redirect_http_to_https: bool,
    #[serde(rename = "enable_proxy_protocol")]
    pub enable_proxy_protocol: bool,
    #[serde(rename = "enable_backend_keepalive")]
    pub enable_backend_keepalive: bool,
    #[serde(rename = "vpc_uuid")]
    pub vpc_uuid: String,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardingRule {
    #[serde(rename = "entry_protocol")]
    pub entry_protocol: String,
    #[serde(rename = "entry_port")]
    pub entry_port: i64,
    #[serde(rename = "target_protocol")]
    pub target_protocol: String,
    #[serde(rename = "target_port")]
    pub target_port: i64,
    #[serde(rename = "certificate_id")]
    pub certificate_id: String,
    #[serde(rename = "tls_passthrough")]
    pub tls_passthrough: bool,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheck {
    pub protocol: String,
    pub port: i64,
    pub path: String,
    #[serde(rename = "check_interval_seconds")]
    pub check_interval_seconds: i64,
    #[serde(rename = "response_timeout_seconds")]
    pub response_timeout_seconds: i64,
    #[serde(rename = "healthy_threshold")]
    pub healthy_threshold: i64,
    #[serde(rename = "unhealthy_threshold")]
    pub unhealthy_threshold: i64,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StickySessions {
    #[serde(rename = "type")]
    pub type_field: String,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Region {
    pub name: String,
    pub slug: String,
    pub sizes: Vec<String>,
    pub features: Vec<String>,
    pub available: bool,
}

use crate::io_models::application::{AdditionalService, PortIo, Protocol};
use serde_derive::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct PublicPortConfig {
    pub path: Option<String>,
    pub path_rewrite: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Port {
    pub long_id: Uuid,
    pub port: u16,
    pub is_default: bool,
    pub name: String,
    pub publicly_accessible: bool,
    pub protocol: Protocol,
    pub service_name: Option<String>,
    pub namespace: Option<String>,
    pub additional_service: Option<AdditionalService>,

    // Override the default matching path. It makes sense only for HTTP and GRPC protocols
    #[serde(default)]
    pub path: Option<String>,
    // Rewrite the path. It makes sense only for HTTP and GRPC protocols
    #[serde(default)]
    pub path_rewrite: Option<String>,
}

impl From<PortIo> for Port {
    fn from(value: PortIo) -> Self {
        Self {
            long_id: value.long_id,
            port: value.port,
            is_default: value.is_default,
            name: value.name,
            publicly_accessible: value.publicly_accessible,
            protocol: value.protocol,
            service_name: value.service_name,
            namespace: value.namespace,
            additional_service: value.additional_service,
            path: value.path,
            path_rewrite: value.path_rewrite,
        }
    }
}
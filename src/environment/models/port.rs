use crate::io_models::application::{AdditionalService, PortIo, Protocol};
use serde_derive::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub enum PortProtocol {
    TCP { public: bool },
    UDP { public: bool },
    HTTP { public: Option<HttpPublicPortConfig> },
    GRPC { public: Option<HttpPublicPortConfig> },
}

impl PortProtocol {
    pub fn protocol(&self) -> Protocol {
        match self {
            PortProtocol::TCP { .. } => Protocol::TCP,
            PortProtocol::UDP { .. } => Protocol::UDP,
            PortProtocol::HTTP { .. } => Protocol::HTTP,
            PortProtocol::GRPC { .. } => Protocol::GRPC,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct HttpPublicPortConfig {
    pub path: String,
    pub path_rewrite: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Port {
    pub long_id: Uuid,
    pub port: u16,
    pub is_default: bool,
    pub name: String,
    pub protocol: PortProtocol,
    pub service_name: Option<String>,
    pub namespace: Option<String>,
    pub additional_service: Option<AdditionalService>,
}

impl Port {
    pub fn is_public(&self) -> bool {
        match &self.protocol {
            PortProtocol::TCP { public } | PortProtocol::UDP { public } => *public,
            PortProtocol::HTTP { public } | PortProtocol::GRPC { public } => public.is_some(),
        }
    }

    pub fn public_path(&self) -> Option<&str> {
        match &self.protocol {
            PortProtocol::HTTP { public } | PortProtocol::GRPC { public } => public.as_ref().map(|p| p.path.as_str()),
            _ => None,
        }
    }

    pub fn public_path_rewrite(&self) -> Option<&str> {
        match &self.protocol {
            PortProtocol::HTTP { public } | PortProtocol::GRPC { public } => {
                public.as_ref().and_then(|p| p.path_rewrite.as_deref())
            }
            _ => None,
        }
    }
}

impl From<PortIo> for Port {
    fn from(value: PortIo) -> Self {
        let public_config = if value.publicly_accessible {
            let path = value.path.unwrap_or("/".to_string());
            let path_rewrite = value.path_rewrite.clone();
            let public_port_config = HttpPublicPortConfig { path, path_rewrite };
            Some(public_port_config)
        } else {
            None
        };

        let protocol = match value.protocol {
            Protocol::HTTP => PortProtocol::HTTP { public: public_config },
            Protocol::GRPC => PortProtocol::GRPC { public: public_config },
            Protocol::TCP => PortProtocol::TCP {
                public: value.publicly_accessible,
            },
            Protocol::UDP => PortProtocol::UDP {
                public: value.publicly_accessible,
            },
        };

        Self {
            long_id: value.long_id,
            port: value.port,
            is_default: value.is_default,
            name: value.name,
            protocol,
            service_name: value.service_name,
            namespace: value.namespace,
            additional_service: value.additional_service,
        }
    }
}

use std::fmt::Display;

use crate::infrastructure::models::cloud_provider::Kind;
use crate::io_models::container::Registry;

pub const AWS_REGISTRY_HOST: &str = "public.ecr.aws";
pub const AWS_REGISTRY_NAME: &str = "r3m4q3r9";

pub const GCP_REGISTRY_HOST: &str = "us-docker.pkg.dev";
pub const GCP_REGISTRY_PROJECT: &str = "qovery";
pub const GCP_REGISTRY_NAME: &str = "ecr-proxy";
pub const GCP_REGISTRY_PATH: &str = "r3m4q3r9";

#[derive(Clone)]
pub enum QoverySourceRegistry {
    AwsEcr {
        host: String,
        registry_name: String,
    },
    GcpArtifact {
        host: String,
        project: String,
        registry_name: String,
        path: String,
    },
}

impl Display for QoverySourceRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            QoverySourceRegistry::AwsEcr { host, registry_name } => write!(f, "{host}/{registry_name}"),
            QoverySourceRegistry::GcpArtifact {
                host,
                project,
                registry_name,
                path,
            } => write!(f, "{host}/{project}/{registry_name}/{path}"),
        }
    }
}

impl From<&Kind> for QoverySourceRegistry {
    fn from(value: &Kind) -> Self {
        match value {
            Kind::Gcp => QoverySourceRegistry::GcpArtifact {
                host: GCP_REGISTRY_HOST.into(),
                project: GCP_REGISTRY_PROJECT.into(),
                registry_name: GCP_REGISTRY_NAME.into(),
                path: GCP_REGISTRY_PATH.into(),
            },
            _ => QoverySourceRegistry::AwsEcr {
                host: AWS_REGISTRY_HOST.into(),
                registry_name: AWS_REGISTRY_NAME.into(),
            },
        }
    }
}

impl From<QoverySourceRegistry> for Registry {
    fn from(val: QoverySourceRegistry) -> Self {
        use url::Url;
        use uuid::Uuid;

        let url = format!("https://{}", val.host());

        Registry::PublicEcr {
            long_id: Uuid::new_v4(),
            url: Url::parse(&url).unwrap(),
        }
    }
}

impl QoverySourceRegistry {
    pub fn host(&self) -> String {
        match self {
            QoverySourceRegistry::AwsEcr { host, .. } => host.into(),
            QoverySourceRegistry::GcpArtifact { host, .. } => host.into(),
        }
    }

    // Does not include host
    pub fn image_path(&self, image_name: &str) -> String {
        match &self {
            QoverySourceRegistry::AwsEcr { registry_name, .. } => {
                format!("{}/{image_name}", registry_name)
            }
            QoverySourceRegistry::GcpArtifact {
                project,
                registry_name,
                path,
                ..
            } => format!("{}/{}/{}/{image_name}", project, registry_name, path),
        }
    }
    // Includes host
    pub fn image_full_path(&self, image_name: &str) -> String {
        format!("{self}/{image_name}")
    }
}

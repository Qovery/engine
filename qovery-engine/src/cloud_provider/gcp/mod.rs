use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::{CloudProvider, CloudProviderError, Kind};

pub struct GCP {
    name: String,
    p12_file_content: String,
}

impl CloudProvider for GCP {
    fn kind(&self) -> Kind {
        Kind::GCP
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn is_valid(&self) -> Result<(), CloudProviderError> {
        Ok(())
    }

    fn kubernetes_clusters(self) -> Result<Vec<Box<dyn Kubernetes>>, CloudProviderError> {
        Ok(vec![])
    }
}

impl GCP {
    pub fn new(name: &str, p12_file_content: &str) -> Self {
        GCP {
            name: name.to_string(),
            p12_file_content: p12_file_content.to_string(),
        }
    }
}

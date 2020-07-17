use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::{CloudProvider, CloudProviderError, Kind};
use std::any::Any;

pub struct GCP {
    execution_id: String,
    id: String,
    name: String,
    p12_file_content: String,
}

impl CloudProvider for GCP {
    fn execution_id(&self) -> &str {
        unimplemented!()
    }

    fn kind(&self) -> Kind {
        Kind::GCP
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn is_valid(&self) -> Result<(), CloudProviderError> {
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl GCP {
    pub fn new(execution_id: &str, id: &str, name: &str, p12_file_content: &str) -> Self {
        GCP {
            execution_id: execution_id.to_string(),
            id: id.to_string(),
            name: name.to_string(),
            p12_file_content: p12_file_content.to_string(),
        }
    }
}

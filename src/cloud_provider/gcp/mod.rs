use crate::cloud_provider::error::CloudProviderError;
use crate::cloud_provider::{
    CloudProvider, CloudProviderName, Kubernetes, Service, StatefulService,
};

pub struct GCP {
    p12_file_content: String,
}

impl CloudProvider for GCP {
    fn name(&self) -> CloudProviderName {
        CloudProviderName::GCP
    }

    fn is_valid(&self) -> Result<(), CloudProviderError> {
        Ok(())
    }

    fn kubernetes_clusters(self) -> Result<Vec<Box<dyn Kubernetes>>, CloudProviderError> {
        Ok(vec![])
    }
}

impl GCP {
    pub fn new(p12_file_content: String) -> Self {
        unimplemented!()
    }
}

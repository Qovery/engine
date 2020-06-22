use crate::cloud_provider::error::CloudProviderError;
use crate::cloud_provider::{
    CloudProvider, CloudProviderName, Kubernetes, Service, StatefulService,
};

pub struct GCP {
    p12_file_content: String,
    kubernetes: Box<dyn Kubernetes>,
}

impl CloudProvider for GCP {
    fn name(&self) -> CloudProviderName {
        CloudProviderName::GCP
    }

    fn is_valid(&self) -> Result<(), CloudProviderError> {
        Ok(())
    }

    fn kubernetes(self) -> Box<dyn Kubernetes> {
        self.kubernetes
    }

    fn services(&self) -> Vec<Box<dyn Service>> {
        vec![]
    }

    fn create_service(&self, service: Box<dyn StatefulService>) {
        unimplemented!()
    }
}

impl GCP {
    pub fn new(p12_file_content: String) -> Self {
        unimplemented!()
    }
}

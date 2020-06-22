use crate::cloud_provider::{
    CloudProvider, CloudProviderName, Kubernetes, Service, StatefulService,
};
use crate::error::QResult;

pub struct GCP<'a> {
    p12_file_content: &'a str,
    kubernetes: Box<dyn Kubernetes>,
}

impl<'a> CloudProvider for GCP<'a> {
    fn name(&self) -> CloudProviderName {
        CloudProviderName::GCP
    }

    fn region(&self) -> String {
        unimplemented!()
    }

    fn is_valid(&self) -> QResult<()> {
        Ok(())
    }

    fn on_create(&self) {
        println!("on_create GCP");
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

impl<'a> GCP<'a> {
    pub fn new(p12_file_content: &'a str) -> Self {
        unimplemented!()
    }
}

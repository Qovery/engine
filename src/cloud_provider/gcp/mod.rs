use crate::cloud_provider::{CloudProvider, Kubernetes, Service, StatefulService};

pub struct GCP<'a> {
    p12_file_content: &'a str,
    kubernetes: Box<dyn Kubernetes>,
}

impl<'a> CloudProvider<'a> for GCP<'a> {
    fn name(&self) -> &'a str {
        "gcp"
    }

    fn region(&self) -> &'a str {
        unimplemented!()
    }

    fn is_valid(&self) -> bool {
        true
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

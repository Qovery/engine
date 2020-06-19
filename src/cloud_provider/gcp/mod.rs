use crate::cloud_provider::{CloudProvider, Kubernetes, Service, StatefulService};

pub struct GCP<'a, K>
where
    K: Kubernetes,
{
    p12_file_content: &'a str,
    kubernetes: K,
}

impl<'a, K> CloudProvider<'a, K> for GCP<'a, K>
where
    K: Kubernetes,
{
    fn name(&self) -> &'a str {
        unimplemented!()
    }

    fn region(&self) -> &'a str {
        unimplemented!()
    }

    fn is_valid(&self) -> bool {
        unimplemented!()
    }

    fn on_create(&self) {
        println!("on_create GCP");
    }

    fn kubernetes(&self) -> &K {
        &self.kubernetes
    }

    fn services(&self) -> Vec<Box<dyn Service>> {
        vec![]
    }

    fn create_service(&self, service: Box<dyn StatefulService<K>>) {
        unimplemented!()
    }
}

impl<'a, K> GCP<'a, K>
where
    K: Kubernetes,
{
    pub fn new(p12_file_content: &'a str) -> Self {
        unimplemented!()
    }
}

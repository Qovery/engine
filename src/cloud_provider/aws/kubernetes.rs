use crate::cloud_provider::aws::AWS;
use crate::cloud_provider::{
    Create, DatabaseType, Kubernetes, Service, ServiceType, StatefulService,
};

pub struct EKS<'a> {
    pub name: &'a str,
    pub version: &'a str,
}

impl<'a> Kubernetes for EKS<'a> {
    fn name(&self) -> &str {
        self.name
    }

    fn version(&self) -> &str {
        self.version
    }

    fn on_create(&self) {
        unimplemented!()
    }

    fn on_upgrade(&self) {
        unimplemented!()
    }

    fn on_downgrade(&self) {
        unimplemented!()
    }

    fn on_delete(&self) {
        unimplemented!()
    }

    fn create_namespace(&self) {
        unimplemented!()
    }

    fn services(&self) -> &Vec<Box<dyn Service>> {
        unimplemented!()
    }
}

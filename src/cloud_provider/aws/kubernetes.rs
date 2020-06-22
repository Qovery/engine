use crate::cloud_provider::aws::AWS;
use crate::cloud_provider::{
    Create, DatabaseType, Kubernetes, Service, ServiceType, StatefulService,
};
use rusoto_core::Region;
use std::str::FromStr;

pub struct EKS {
    pub name: String,
    pub version: String,
    pub region: Region,
}

impl<'a> EKS {
    pub fn new(name: &'a str, version: &'a str, region: &'a str) -> Self {
        EKS {
            name: name.to_string(),
            version: version.to_string(),
            region: Region::from_str(region).unwrap(),
        }
    }
}

impl Kubernetes for EKS {
    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn version(&self) -> &str {
        self.version.as_str()
    }

    fn region(&self) -> &str {
        self.region.name()
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

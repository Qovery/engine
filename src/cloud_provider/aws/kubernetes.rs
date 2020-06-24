use crate::cloud_provider::aws::AWS;
use crate::cloud_provider::error::KubernetesError;
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

    fn is_valid(&self) -> Result<(), KubernetesError> {
        Ok(())
    }

    fn on_create(&self) -> Result<(), KubernetesError> {
        Ok(())
    }

    fn on_create_error(&self) -> Result<(), KubernetesError> {
        unimplemented!()
    }

    fn on_upgrade(&self) -> Result<(), KubernetesError> {
        unimplemented!()
    }

    fn on_upgrade_error(&self) -> Result<(), KubernetesError> {
        unimplemented!()
    }

    fn on_downgrade(&self) -> Result<(), KubernetesError> {
        unimplemented!()
    }

    fn on_downgrade_error(&self) -> Result<(), KubernetesError> {
        unimplemented!()
    }

    fn on_delete(&self) -> Result<(), KubernetesError> {
        unimplemented!()
    }

    fn on_delete_error(&self) -> Result<(), KubernetesError> {
        unimplemented!()
    }

    fn create_namespace(&self) -> Result<(), KubernetesError> {
        unimplemented!()
    }

    fn services(&self) -> Result<Vec<Box<dyn Service>>, KubernetesError> {
        unimplemented!()
    }

    fn create_service(&self, service: Box<dyn StatefulService>) -> Result<(), KubernetesError> {
        unimplemented!()
    }
}

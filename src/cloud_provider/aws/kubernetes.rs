use crate::cloud_provider::aws::AWS;
use crate::cloud_provider::error::KubernetesError;
use crate::cloud_provider::{
    CloudProvider, Create, DatabaseType, Kubernetes, Service, ServiceType, StatefulService,
};
use rusoto_core::Region;
use std::str::FromStr;

pub struct EKS<'a> {
    name: String,
    version: String,
    region: Region,
    cloud_provider: &'a dyn CloudProvider,
}

impl<'a> EKS<'a> {
    pub fn new(
        name: &str,
        version: &str,
        region: &str,
        cloud_provider: &'a dyn CloudProvider,
    ) -> Self {
        EKS {
            name: name.to_string(),
            version: version.to_string(),
            region: Region::from_str(region).unwrap(),
            cloud_provider,
        }
    }
}

impl<'a> Kubernetes for EKS<'a> {
    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn version(&self) -> &str {
        self.version.as_str()
    }

    fn region(&self) -> &str {
        self.region.name()
    }

    fn cloud_provider(&self) -> &dyn CloudProvider {
        self.cloud_provider
    }

    fn is_valid(&self) -> Result<(), KubernetesError> {
        Ok(())
    }

    fn on_create(&self) -> Result<(), KubernetesError> {
        // TODO pierre
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

    fn create_service(&self, service: &dyn Service) -> Result<(), KubernetesError> {
        unimplemented!()
    }

    fn delete_service(&self, service: &dyn Service) -> Result<(), KubernetesError> {
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::aws::kubernetes::EKS;
    use crate::cloud_provider::aws::AWS;
    use crate::cloud_provider::CloudProvider;

    #[test]
    fn test() {
        let aws = AWS::new(
            "AKIAZ4KMLSYJLRGNNFNI",
            "8dRLHmIbK1BiZhaz0pLc38MRPQomee0bF5Hz8eG/",
        );

        match aws.is_valid() {
            Err(err) => panic!("something goes wrong with the connection to AWS"),
            _ => {}
        }

        let eks = EKS::new("test-cluster", "1.16", "eu-west-3", &aws);
    }
}

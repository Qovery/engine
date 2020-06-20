mod databases;
pub mod kubernetes;

use crate::cloud_provider::aws::kubernetes::EKS;
use crate::cloud_provider::{CloudProvider, Kubernetes, Service, StatefulService};

pub struct AWS<'a> {
    pub access_key_id: &'a str,
    pub secret_access_key: &'a str,
    pub kubernetes: Box<dyn Kubernetes>,
}

impl<'a> CloudProvider<'a> for AWS<'a> {
    fn name(&self) -> &'a str {
        "aws"
    }

    fn region(&self) -> &'a str {
        unimplemented!()
    }

    fn is_valid(&self) -> bool {
        true
    }

    fn on_create(&self) {
        println!("on_create AWS");
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

#[cfg(test)]
mod tests {
    use crate::cloud_provider::aws::kubernetes::EKS;
    use crate::cloud_provider::aws::AWS;
    use crate::cloud_provider::{CloudProvider, Kubernetes, ServiceType};

    #[test]
    fn aws() {
        let eks = Box::new(EKS {
            id: "",
            name: "",
            version: "",
        });

        let aws = AWS {
            access_key_id: "",
            secret_access_key: "",
            kubernetes: eks,
        };

        aws.services().iter().for_each(|x| {
            match x.service_type() {
                ServiceType::Application => {}
                ServiceType::Database(db) => {}
            };
        });
    }
}

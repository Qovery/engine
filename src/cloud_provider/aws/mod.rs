mod databases;
pub mod kubernetes;

use crate::cloud_provider::aws::kubernetes::EKS;
use crate::cloud_provider::{CloudProvider, Kubernetes, Service, StatefulService};

pub struct AWS<'a, K>
where
    K: Kubernetes,
{
    pub access_key_id: &'a str,
    pub secret_access_key: &'a str,
    pub kubernetes: K,
}

impl<'a, K> CloudProvider<'a, K> for AWS<'a, K>
where
    K: Kubernetes,
{
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

impl<'a, K> AWS<'a, K>
where
    K: Kubernetes,
{
    pub fn new(access_key_id: &'a str, secret_access_key: &'a str) -> Self {
        let kubernetes = K::new();

        AWS {
            access_key_id,
            secret_access_key,
            kubernetes,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::aws::kubernetes::EKS;
    use crate::cloud_provider::aws::AWS;
    use crate::cloud_provider::{CloudProvider, Kubernetes, ServiceType};

    #[test]
    fn aws() {
        let aws = AWS {
            access_key_id: "",
            secret_access_key: "",
            kubernetes: EKS::new(),
        };

        aws.services().iter().for_each(|x| {
            match x.service_type() {
                ServiceType::Application => {}
                ServiceType::Database(db) => {}
            };
        });
    }
}

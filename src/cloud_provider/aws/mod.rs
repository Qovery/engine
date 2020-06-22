mod databases;
pub mod kubernetes;

use crate::cloud_provider::aws::kubernetes::EKS;
use crate::cloud_provider::{
    CloudProvider, CloudProviderName, Kubernetes, Service, StatefulService,
};
use crate::error::{QError, QResult};
use crate::runtime::async_run;
use rusoto_core::{Client, HttpClient, Region, RusotoError};
use rusoto_credential::{AwsCredentials, StaticProvider};
use rusoto_eks::{
    DescribeClusterRequest, Eks, EksClient, ListClustersError, ListClustersRequest,
    ListClustersResponse,
};
use rusoto_sts::{GetCallerIdentityRequest, Sts, StsClient};
use std::str::FromStr;

pub struct AWS {
    access_key_id: String,
    secret_access_key: String,
    region: Region,
    kubernetes: Box<dyn Kubernetes>,
}

impl<'a> AWS {
    pub fn new(
        access_key_id: &'a str,
        secret_access_key: &'a str,
        region: &'a str,
        kubernetes: Box<dyn Kubernetes>,
    ) -> Self {
        AWS {
            access_key_id: access_key_id.to_string(),
            secret_access_key: secret_access_key.to_string(),
            region: Region::from_str(region).unwrap(),
            kubernetes,
        }
    }

    pub fn credentials(&self) -> StaticProvider {
        StaticProvider::new(
            self.access_key_id.to_string(),
            self.secret_access_key.to_string(),
            None,
            None,
        )
    }

    pub fn client(&self) -> Client {
        Client::new_with(self.credentials(), HttpClient::new().unwrap())
    }
}

impl CloudProvider for AWS {
    fn name(&self) -> CloudProviderName {
        CloudProviderName::AWS
    }

    fn region(&self) -> String {
        self.region.name().to_string()
    }

    fn is_valid(&self) -> QResult<()> {
        let client = StsClient::new_with_client(self.client(), self.region.clone());
        let s = async_run(client.get_caller_identity(GetCallerIdentityRequest::default()));

        match s {
            Ok(x) => Ok(()),
            Err(err) => Err(QError::from(err)),
        }
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
            name: "",
            version: "",
        });

        let aws = AWS::new("", "", "us-east-2", eks);
        assert_eq!(aws.is_valid().is_ok(), false);

        aws.services().iter().for_each(|x| {
            match x.service_type() {
                ServiceType::Application => {}
                ServiceType::Database(db) => {}
            };
        });
    }
}

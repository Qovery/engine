use std::str::FromStr;

use rusoto_core::{Client, HttpClient, Region, RusotoError};
use rusoto_credential::{AwsCredentials, StaticProvider};
use rusoto_eks::{
    DescribeClusterRequest, Eks, EksClient, ListClustersError, ListClustersRequest,
    ListClustersResponse,
};
use rusoto_sts::{GetCallerIdentityRequest, Sts, StsClient};

use crate::cloud_provider::aws::kubernetes::EKS;
use crate::cloud_provider::error::CloudProviderError;
use crate::cloud_provider::{
    CloudProvider, CloudProviderName, Create, Kubernetes, Service, StatefulService,
};
use crate::error::ConfigurationError;
use crate::runtime::async_run;

pub mod databases;
pub mod kubernetes;

pub struct AWS {
    access_key_id: String,
    secret_access_key: String,
}

impl<'a> AWS {
    pub fn new(access_key_id: &'a str, secret_access_key: &'a str) -> Self {
        AWS {
            access_key_id: access_key_id.to_string(),
            secret_access_key: secret_access_key.to_string(),
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

    fn is_valid(&self) -> Result<(), CloudProviderError> {
        let client = StsClient::new_with_client(self.client(), Region::default());
        let s = async_run(client.get_caller_identity(GetCallerIdentityRequest::default()));

        match s {
            Ok(x) => Ok(()),
            Err(err) => Err(CloudProviderError::from(err)),
        }
    }

    fn kubernetes_clusters(self) -> Vec<Box<dyn Kubernetes>> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud_provider::aws::kubernetes::EKS;
    use crate::cloud_provider::aws::AWS;
    use crate::cloud_provider::{CloudProvider, Kubernetes, ServiceType};

    #[test]
    fn aws() {
        let eks = Box::new(EKS::new("", "", ""));

        let aws = AWS::new("", "", eks);
        assert_eq!(aws.is_valid().is_ok(), false);

        aws.services().iter().for_each(|x| {
            match x.service_type() {
                ServiceType::Application => {}
                ServiceType::Database(db) => {}
            };
        });
    }
}

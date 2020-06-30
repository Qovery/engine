use rusoto_core::{Client, HttpClient, Region};
use rusoto_credential::StaticProvider;
use rusoto_sts::{GetCallerIdentityRequest, Sts, StsClient};

use crate::cloud_provider::error::CloudProviderError;
use crate::cloud_provider::{CloudProvider, CloudProviderName, Kubernetes};
use crate::runtime::async_run;

pub mod databases;
pub mod kubernetes;

pub struct AWS {
    access_key_id: String,
    secret_access_key: String,
}

impl AWS {
    pub fn new(access_key_id: &str, secret_access_key: &str) -> Self {
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

    fn kubernetes_clusters(self) -> Result<Vec<Box<dyn Kubernetes>>, CloudProviderError> {
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn aws() {}
}

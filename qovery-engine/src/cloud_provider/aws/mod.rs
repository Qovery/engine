use std::any::Any;

use rusoto_core::{Client, HttpClient, Region};
use rusoto_credential::StaticProvider;
use rusoto_sts::{GetCallerIdentityRequest, Sts, StsClient};

use crate::cloud_provider::kubernetes::Kubernetes;
use crate::cloud_provider::{CloudProvider, CloudProviderError, Kind};
use crate::runtime::async_run;

mod common;

pub mod application;
pub mod databases;
pub mod router;

pub mod kubernetes;

pub struct AWS {
    id: String,
    name: String,
    pub access_key_id: String,
    pub secret_access_key: String,
}

impl AWS {
    pub fn new(id: &str, name: &str, access_key_id: &str, secret_access_key: &str) -> Self {
        AWS {
            id: id.to_string(),
            name: name.to_string(),
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
    fn kind(&self) -> Kind {
        Kind::AWS
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn is_valid(&self) -> Result<(), CloudProviderError> {
        let client = StsClient::new_with_client(self.client(), Region::default());
        let s = async_run(client.get_caller_identity(GetCallerIdentityRequest::default()));

        match s {
            Ok(x) => Ok(()),
            Err(err) => Err(CloudProviderError::from(err)),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn aws() {}
}

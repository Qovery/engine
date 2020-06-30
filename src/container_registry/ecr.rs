use crate::build_platform::Image;
use crate::container_registry::error::ContainerRegistryError;
use crate::container_registry::{ContainerRegistry, PushError, PushResult};
use crate::runtime::async_run;
use rusoto_core::{Client, HttpClient, Region};
use rusoto_credential::StaticProvider;
use rusoto_ecr::{Ecr, EcrClient, ListImagesRequest};
use rusoto_sts::{GetCallerIdentityRequest, Sts, StsClient};
use std::str::FromStr;

pub struct ECR {
    access_key_id: String,
    secret_access_key: String,
    region: Region,
}

impl ECR {
    pub fn new(access_key_id: &str, secret_access_key: &str, region: &str) -> Self {
        ECR {
            access_key_id: access_key_id.to_string(),
            secret_access_key: secret_access_key.to_string(),
            region: Region::from_str(region).unwrap(),
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

    pub fn ecr_client(&self) -> EcrClient {
        EcrClient::new_with_client(self.client(), self.region.clone())
    }
}

impl ContainerRegistry for ECR {
    fn is_valid(&self) -> Result<(), ContainerRegistryError> {
        let client = StsClient::new_with_client(self.client(), Region::default());
        let s = async_run(client.get_caller_identity(GetCallerIdentityRequest::default()));

        match s {
            Ok(x) => Ok(()),
            Err(err) => Err(ContainerRegistryError::from(err)),
        }
    }

    fn on_create(&self) -> Result<(), ContainerRegistryError> {
        unimplemented!()
    }

    fn on_create_error(&self) -> Result<(), ContainerRegistryError> {
        unimplemented!()
    }

    fn on_delete(&self) -> Result<(), ContainerRegistryError> {
        unimplemented!()
    }

    fn on_delete_error(&self) -> Result<(), ContainerRegistryError> {
        unimplemented!()
    }

    fn push(&self, image: Image) -> Result<PushResult, PushError> {
        unimplemented!()
    }

    fn push_error(&self, image: Image) -> Result<PushResult, PushError> {
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {
    use crate::container_registry::ecr::ECR;
    use crate::container_registry::error::ContainerRegistryError;
    use crate::container_registry::ContainerRegistry;

    #[test]
    fn test_is_not_valid() {
        let ecr = ECR::new("fake", "fake", "us-east-2");
        assert_eq!(ecr.is_valid().is_err(), true);
        assert_eq!(
            ecr.is_valid().err().unwrap(),
            ContainerRegistryError::Credentials
        );
    }

    #[test]
    fn test_is_valid() {
        let ecr = ECR::new(
            "AKIAZ4KMLSYJLRGNNFNI",
            "8dRLHmIbK1BiZhaz0pLc38MRPQomee0bF5Hz8eG/",
            "us-east-2",
        );

        assert_eq!(ecr.is_valid().is_ok(), true);
    }
}

use crate::build_platform::Image;
use crate::cmd;
use crate::cmd::CmdError;
use crate::container_registry::error::ContainerRegistryError;
use crate::container_registry::{ContainerRegistry, PushError, PushResult};
use crate::runtime::async_run;
use crate::transaction::CommitError::Push;
use rusoto_core::{Client, HttpClient, Region};
use rusoto_credential::StaticProvider;
use rusoto_ecr::{
    CreateRepositoryRequest, DescribeRepositoriesRequest, Ecr, EcrClient,
    GetAuthorizationTokenRequest, GetAuthorizationTokenResponse, ListImagesRequest,
    PutLifecyclePolicyRequest, Repository,
};
use rusoto_sts::{GetCallerIdentityRequest, Sts, StsClient};
use std::str::FromStr;

pub struct ECR {
    access_key_id: String,
    secret_access_key: String,
    region: Region,
    name: String,
}

impl ECR {
    pub fn new(access_key_id: &str, secret_access_key: &str, region: &str, name: &str) -> Self {
        ECR {
            access_key_id: access_key_id.to_string(),
            secret_access_key: secret_access_key.to_string(),
            region: Region::from_str(region).unwrap(),
            name: name.to_string(),
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

    pub fn get_repository(&self) -> Option<Repository> {
        let mut drr = DescribeRepositoriesRequest::default();
        drr.repository_names = Some(vec![self.name.clone()]);

        let r = async_run(self.ecr_client().describe_repositories(drr));

        match r {
            Err(_) => None,
            Ok(res) => match res.repositories {
                // assume there is only one repository returned - why? Because we set only one repository_names above
                Some(repositories) => repositories.into_iter().next(),
                _ => None,
            },
        }
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
        info!("ECR.on_create() called for {}", self.name);

        // check if the repository already exists
        if self.get_repository().is_some() {
            info!("ECR repository {} already exists", self.name);
            return Ok(());
        }

        info!("ECR create repository {}", self.name);
        let mut crr = CreateRepositoryRequest::default();
        crr.repository_name = self.name.clone();

        let r = async_run(self.ecr_client().create_repository(crr));
        match r {
            Err(err) => return Err(ContainerRegistryError::from(err)),
            _ => {}
        }

        let mut plp = PutLifecyclePolicyRequest::default();
        plp.repository_name = self.name.clone();

        let ecr_policy = r#"
        {
          "rules": [
            {
              "action": {
                "type": "expire"
              },
              "selection": {
                "countType": "sinceImagePushed",
                "countUnit": "days",
                "countNumber": 1,
                "tagStatus": "any"
              },
              "description": "Remove unit test images",
              "rulePriority": 1
            }
          ]
        }
        "#;

        plp.lifecycle_policy_text = ecr_policy.to_string();

        let r = async_run(self.ecr_client().put_lifecycle_policy(plp));

        match r {
            Err(err) => Err(ContainerRegistryError::from(err)),
            _ => Ok(()),
        }
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
        let r = async_run(
            self.ecr_client()
                .get_authorization_token(GetAuthorizationTokenRequest::default()),
        );

        let (access_token, password, endpoint_url) = match r {
            Ok(t) => match t.authorization_data {
                Some(authorization_data) => {
                    let ad = authorization_data.first().unwrap();
                    let b64_token = ad.authorization_token.as_ref().unwrap();

                    let decoded_token = base64::decode(b64_token).unwrap();
                    let token = std::str::from_utf8(decoded_token.as_slice()).unwrap();

                    let s_token: Vec<&str> = token.split(":").collect::<Vec<_>>();

                    (
                        s_token.first().unwrap().to_string(),
                        s_token.get(1).unwrap().to_string(),
                        ad.clone().proxy_endpoint.unwrap(),
                    )
                }
                None => return Err(PushError::RepositoryInitFailure),
            },
            _ => return Err(PushError::RepositoryInitFailure),
        };

        let repository = match self.get_repository() {
            Some(r) => r,
            None => return Err(PushError::RepositoryInitFailure),
        };

        match cmd::exec(
            "docker",
            vec![
                "login",
                "-u",
                access_token.as_str(),
                "-p",
                password.as_str(),
                endpoint_url.as_str(),
            ],
        ) {
            Err(err) => match err {
                CmdError::Io(err) => panic!(err),
                CmdError::Exec(exit_status) => return Err(PushError::CredentialsError),
            },
            _ => {}
        };

        let dest = format!(
            "{}/{}",
            repository.repository_uri.unwrap(),
            image.name_with_tag().as_str()
        );

        match cmd::exec("docker", vec!["tag", dest.as_str(), self.name.as_str()]) {
            Err(err) => match err {
                CmdError::Io(err) => panic!(err),
                CmdError::Exec(exit_status) => return Err(PushError::ImageTagFailed),
            },
            _ => {}
        };

        match cmd::exec("docker", vec!["push", dest.as_str()]) {
            Err(err) => match err {
                CmdError::Io(err) => panic!(err),
                CmdError::Exec(exit_status) => return Err(PushError::ImagePushFailed),
            },
            _ => {}
        };

        Ok(PushResult { image })
    }

    fn push_error(&self, image: Image) -> Result<PushResult, PushError> {
        unimplemented!()
    }
}

#[cfg(test)]
mod tests {
    use crate::build_platform::Image;
    use crate::container_registry::ecr::ECR;
    use crate::container_registry::error::ContainerRegistryError;
    use crate::container_registry::ContainerRegistry;

    #[test]
    fn test_is_not_valid() {
        let ecr = ECR::new("fake", "fake", "us-east-2", "test-repo-name");
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
            "test-repo-name",
        );

        assert_eq!(ecr.is_valid().is_ok(), true);
    }

    #[test]
    fn test_create_repository() {
        let ecr = ECR::new(
            "AKIAZ4KMLSYJLRGNNFNI",
            "8dRLHmIbK1BiZhaz0pLc38MRPQomee0bF5Hz8eG/",
            "us-east-2",
            "test-repo-name",
        );

        assert_eq!(ecr.on_create().is_ok(), true);
    }
}

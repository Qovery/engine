use std::str::FromStr;

use rusoto_core::{Client, HttpClient, Region};
use rusoto_credential::StaticProvider;
use rusoto_ecr::{
    CreateRepositoryRequest, DescribeRepositoriesRequest, Ecr, EcrClient,
    GetAuthorizationTokenRequest, GetAuthorizationTokenResponse, ListImagesRequest,
    PutLifecyclePolicyRequest, Repository,
};
use rusoto_sts::{GetCallerIdentityRequest, Sts, StsClient};

use crate::build_platform::Image;
use crate::cmd;
use crate::cmd::CmdError;
use crate::container_registry::{
    ContainerRegistry, ContainerRegistryError, Kind, PushError, PushResult,
};
use crate::runtime::async_run;
use crate::transaction::CommitError::Push;

pub struct ECR {
    id: String,
    name: String,
    access_key_id: String,
    secret_access_key: String,
    region: Region,
}

impl ECR {
    pub fn new(
        id: &str,
        name: &str,
        access_key_id: &str,
        secret_access_key: &str,
        region: &str,
    ) -> Self {
        ECR {
            id: id.to_string(),
            name: name.to_string(),
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

    pub fn get_repository(&self, image_name: &str) -> Option<Repository> {
        let mut drr = DescribeRepositoriesRequest::default();
        drr.repository_names = Some(vec![image_name.to_string()]);

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

    pub fn get_or_create_repository(
        &self,
        image: &Image,
    ) -> Result<Repository, ContainerRegistryError> {
        // check if the repository already exists
        let repository = self.get_repository(image.name.as_str());
        if repository.is_some() {
            info!("ECR repository {} already exists", image.name.as_str());
            return Ok(repository.unwrap());
        }

        info!("ECR create repository {}", image.name.as_str());
        let mut crr = CreateRepositoryRequest::default();
        crr.repository_name = image.name.clone();

        let r = async_run(self.ecr_client().create_repository(crr));
        match r {
            Err(err) => return Err(ContainerRegistryError::from(err)),
            _ => {}
        }

        let mut plp = PutLifecyclePolicyRequest::default();
        plp.repository_name = image.name.clone();

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
            _ => Ok(self.get_repository(image.name.as_str()).unwrap()),
        }
    }
}

impl ContainerRegistry for ECR {
    fn kind(&self) -> Kind {
        Kind::ECR
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn is_valid(&self) -> Result<(), ContainerRegistryError> {
        let client = StsClient::new_with_client(self.client(), Region::default());
        let s = async_run(client.get_caller_identity(GetCallerIdentityRequest::default()));

        match s {
            Ok(x) => Ok(()),
            Err(err) => Err(ContainerRegistryError::from(err)),
        }
    }

    fn on_create(&self) -> Result<(), ContainerRegistryError> {
        info!("ECR.on_create() called");
        Ok(())
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

        let repository = match self.get_or_create_repository(&image) {
            Ok(r) => r,
            _ => return Err(PushError::RepositoryInitFailure),
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
            "{}:{}",
            repository.repository_uri.unwrap(),
            image.tag.as_str()
        );

        // READ https://docs.aws.amazon.com/AmazonECR/latest/userguide/docker-push-ecr-image.html
        // docker tag e9ae3c220b23 aws_account_id.dkr.ecr.region.amazonaws.com/my-web-app
        match cmd::exec(
            "docker",
            vec!["tag", image.name_with_tag().as_str(), dest.as_str()],
        ) {
            Err(err) => match err {
                CmdError::Io(err) => panic!(err),
                CmdError::Exec(exit_status) => return Err(PushError::ImageTagFailed),
            },
            _ => {}
        };

        // docker push aws_account_id.dkr.ecr.region.amazonaws.com/my-web-app
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
    use crate::container_registry::{ContainerRegistry, ContainerRegistryError};

    #[test]
    fn test_is_not_valid() {
        let ecr = ECR::new("123-abc", "my-ecr", "fake", "fake", "us-east-2");
        assert_eq!(ecr.is_valid().is_err(), true);
        assert_eq!(
            ecr.is_valid().err().unwrap(),
            ContainerRegistryError::Credentials
        );
    }

    #[test]
    fn test_is_valid() {
        let ecr = ECR::new(
            "123-abc",
            "my-ecr",
            "AKIAZ4KMLSYJLRGNNFNI",
            "8dRLHmIbK1BiZhaz0pLc38MRPQomee0bF5Hz8eG/",
            "us-east-2",
        );

        assert_eq!(ecr.is_valid().is_ok(), true);
    }
}

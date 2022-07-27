#![allow(clippy::field_reassign_with_default)]

use std::str::FromStr;

use rusoto_core::{Client, HttpClient, Region, RusotoError};
use rusoto_credential::StaticProvider;
use rusoto_ecr::{
    CreateRepositoryRequest, DescribeImagesRequest, DescribeRepositoriesError, DescribeRepositoriesRequest, Ecr,
    EcrClient, GetAuthorizationTokenRequest, ImageDetail, ImageIdentifier, PutLifecyclePolicyRequest, Repository,
};
use rusoto_sts::{GetCallerIdentityRequest, Sts, StsClient};

use crate::build_platform::Image;
use crate::container_registry::errors::ContainerRegistryError;
use crate::container_registry::{ContainerRegistry, ContainerRegistryInfo, Kind};
use crate::events::{EngineEvent, EventMessage, GeneralStep, Stage};
use crate::io_models::context::Context;
use crate::io_models::progress_listener::{
    Listener, Listeners, ListenersHelper, ProgressInfo, ProgressLevel, ProgressScope,
};
use crate::logger::Logger;
use crate::runtime::block_on;
use retry::delay::Fixed;
use retry::Error::Operation;
use retry::OperationResult;
use serde_json::json;
use url::Url;

pub struct ECR {
    context: Context,
    id: String,
    name: String,
    access_key_id: String,
    secret_access_key: String,
    region: Region,
    registry_info: Option<ContainerRegistryInfo>,
    listeners: Listeners,
    logger: Box<dyn Logger>,
}

impl ECR {
    pub fn new(
        context: Context,
        id: &str,
        name: &str,
        access_key_id: &str,
        secret_access_key: &str,
        region: &str,
        listener: Listener,
        logger: Box<dyn Logger>,
    ) -> Result<Self, ContainerRegistryError> {
        let mut cr = ECR {
            context,
            id: id.to_string(),
            name: name.to_string(),
            access_key_id: access_key_id.to_string(),
            secret_access_key: secret_access_key.to_string(),
            region: Region::from_str(region).unwrap(),
            registry_info: None,
            listeners: vec![listener],
            logger,
        };

        let credentials = cr.get_credentials()?;
        let mut registry_url = Url::parse(credentials.endpoint_url.as_str()).unwrap();
        let _ = registry_url.set_username(&credentials.access_token);
        let _ = registry_url.set_password(Some(&credentials.password));

        cr.log_info(format!("🔓 Login to ECR registry {}", credentials.endpoint_url));
        cr.context
            .docker
            .login(&registry_url)
            .map_err(|_err| ContainerRegistryError::InvalidCredentials)?;

        let registry_info = ContainerRegistryInfo {
            endpoint: registry_url,
            registry_name: cr.name.to_string(),
            registry_docker_json_config: None,
            get_image_name: Box::new(|img_name| img_name.to_string()),
            get_repository_name: Box::new(|imag_name| imag_name.to_string()),
        };

        cr.registry_info = Some(registry_info);
        cr.is_credentials_valid()?;
        Ok(cr)
    }

    pub fn log_info(&self, msg: String) {
        self.logger.log(EngineEvent::Info(
            self.get_event_details(Stage::General(GeneralStep::ValidateSystemRequirements)),
            EventMessage::new_from_safe(msg.clone()),
        ));

        let lh = ListenersHelper::new(&self.listeners);
        lh.deployment_in_progress(ProgressInfo::new(
            ProgressScope::Environment {
                id: self.context.execution_id().to_string(),
            },
            ProgressLevel::Info,
            Some(msg),
            self.context.execution_id(),
        ));
    }

    pub fn credentials(&self) -> StaticProvider {
        StaticProvider::new(self.access_key_id.to_string(), self.secret_access_key.to_string(), None, None)
    }

    pub fn client(&self) -> Client {
        Client::new_with(self.credentials(), HttpClient::new().unwrap())
    }

    pub fn ecr_client(&self) -> EcrClient {
        EcrClient::new_with_client(self.client(), self.region.clone())
    }

    fn get_repository(&self, repository_name: &str) -> Option<Repository> {
        let mut drr = DescribeRepositoriesRequest::default();
        drr.repository_names = Some(vec![repository_name.to_string()]);

        let r = block_on(self.ecr_client().describe_repositories(drr));

        match r {
            Err(_) => None,
            Ok(res) => match res.repositories {
                // assume there is only one repository returned - why? Because we set only one repository_names above
                Some(repositories) => repositories.into_iter().next(),
                _ => None,
            },
        }
    }

    fn get_image(&self, image: &Image) -> Option<ImageDetail> {
        let mut dir = DescribeImagesRequest::default();
        dir.repository_name = image.name();

        let mut image_identifier = ImageIdentifier::default();
        image_identifier.image_tag = Some(image.tag.to_string());
        dir.image_ids = Some(vec![image_identifier]);

        let r = block_on(self.ecr_client().describe_images(dir));

        match r {
            Err(_) => None,
            Ok(res) => match res.image_details {
                // assume there is only one repository returned - why? Because we set only one repository_names above
                Some(image_details) => image_details.into_iter().next(),
                _ => None,
            },
        }
    }

    fn create_repository(&self, repository_name: &str) -> Result<Repository, ContainerRegistryError> {
        let container_registry_request = DescribeRepositoriesRequest {
            repository_names: Some(vec![repository_name.to_string()]),
            ..Default::default()
        };
        let crr = CreateRepositoryRequest {
            repository_name: repository_name.to_string(),
            ..Default::default()
        };

        // ensure repository is created
        // need to do all this checks and retry because of several issues encountered like: 200 API response code while repo is not created
        let repo_created = retry::retry(Fixed::from_millis(5000).take(24), || {
            let repositories = block_on(
                self.ecr_client()
                    .describe_repositories(container_registry_request.clone()),
            );
            match repositories {
                // Repo already exist, so ok
                Ok(_) => OperationResult::Ok(()),

                // Repo does not exist, so creating it
                Err(RusotoError::Service(DescribeRepositoriesError::RepositoryNotFound(_))) => {
                    if let Err(err) = block_on(self.ecr_client().create_repository(crr.clone())) {
                        OperationResult::Retry(Err(ContainerRegistryError::CannotCreateRepository {
                            registry_name: self.name.to_string(),
                            repository_name: repository_name.to_string(),
                            raw_error_message: err.to_string(),
                        }))
                    } else {
                        // The Repo should be created at this point, but we want to verify that
                        // the describe/list return it now. we want to reloop so return a retry instead of a ok
                        OperationResult::Retry(Err(ContainerRegistryError::CannotCreateRepository {
                            registry_name: self.name.to_string(),
                            repository_name: repository_name.to_string(),
                            raw_error_message: "Retry to check repository exist".to_string(),
                        }))
                    }
                }

                // Unknown error, so retries ¯\_(ツ)_/¯
                Err(err) => OperationResult::Retry(Err(ContainerRegistryError::CannotCreateRepository {
                    registry_name: self.name.to_string(),
                    repository_name: repository_name.to_string(),
                    raw_error_message: err.to_string(),
                })),
            }
        });

        match repo_created {
            Ok(_) => {}
            Err(Operation { error, .. }) => return error,
            Err(retry::Error::Internal(e)) => {
                return Err(ContainerRegistryError::CannotCreateRepository {
                    registry_name: self.name.to_string(),
                    repository_name: repository_name.to_string(),
                    raw_error_message: e,
                })
            }
        };

        // apply retention policy
        let retention_policy_in_days = match self.context.is_test_cluster() {
            true => 1,
            false => 365,
        };
        let lifecycle_policy_text = json!({
          "rules": [
            {
              "action": {
                "type": "expire"
              },
              "selection": {
                "countType": "sinceImagePushed",
                "countUnit": "days",
                "countNumber": retention_policy_in_days,
                "tagStatus": "any"
              },
              "description": "Images retention policy",
              "rulePriority": 1
            }
          ]
        });

        let plp = PutLifecyclePolicyRequest {
            repository_name: repository_name.to_string(),
            lifecycle_policy_text: lifecycle_policy_text.to_string(),
            ..Default::default()
        };

        match block_on(self.ecr_client().put_lifecycle_policy(plp)) {
            Err(err) => Err(ContainerRegistryError::CannotSetRepositoryLifecyclePolicy {
                registry_name: self.name.to_string(),
                repository_name: repository_name.to_string(),
                raw_error_message: err.to_string(),
            }),
            _ => Ok(self.get_repository(repository_name).expect("cannot get repository")),
        }
    }

    fn get_or_create_repository(&self, repository_name: &str) -> Result<Repository, ContainerRegistryError> {
        self.log_info(format!("🗂️ Provisioning container repository {}", repository_name));

        // check if the repository already exists
        let repository = self.get_repository(repository_name);
        if let Some(repo) = repository {
            return Ok(repo);
        }

        self.create_repository(repository_name)
    }

    fn get_credentials(&self) -> Result<ECRCredentials, ContainerRegistryError> {
        let r = block_on(
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

                    let s_token: Vec<&str> = token.split(':').collect::<Vec<_>>();

                    (
                        s_token.first().unwrap().to_string(),
                        s_token.get(1).unwrap().to_string(),
                        ad.clone().proxy_endpoint.unwrap(),
                    )
                }
                None => {
                    return Err(ContainerRegistryError::CannotGetCredentials);
                }
            },
            _ => {
                return Err(ContainerRegistryError::CannotGetCredentials);
            }
        };

        Ok(ECRCredentials::new(access_token, password, endpoint_url))
    }

    fn is_credentials_valid(&self) -> Result<(), ContainerRegistryError> {
        let client = StsClient::new_with_client(self.client(), Region::default());
        let s = block_on(client.get_caller_identity(GetCallerIdentityRequest::default()));

        match s {
            Ok(_) => Ok(()),
            Err(_) => Err(ContainerRegistryError::InvalidCredentials),
        }
    }
}

impl ContainerRegistry for ECR {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::Ecr
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn registry_info(&self) -> &ContainerRegistryInfo {
        // At this point the registry info should be initialize, so unwrap is safe
        self.registry_info.as_ref().unwrap()
    }

    fn create_registry(&self) -> Result<(), ContainerRegistryError> {
        // Nothing to do, ECR require to create only repository
        Ok(())
    }

    fn create_repository(&self, name: &str) -> Result<(), ContainerRegistryError> {
        let _ = self.get_or_create_repository(name)?;
        Ok(())
    }

    fn does_image_exists(&self, image: &Image) -> bool {
        self.get_image(image).is_some()
    }
}

struct ECRCredentials {
    access_token: String,
    password: String,
    endpoint_url: String,
}

impl ECRCredentials {
    fn new(access_token: String, password: String, endpoint_url: String) -> Self {
        ECRCredentials {
            access_token,
            password,
            endpoint_url,
        }
    }
}

#![allow(clippy::field_reassign_with_default)]

use std::collections::HashMap;
use std::str::FromStr;

use rusoto_core::{Client, HttpClient, Region, RusotoError};
use rusoto_credential::StaticProvider;
use rusoto_ecr::{
    BatchDeleteImageRequest, CreateRepositoryRequest, DeleteRepositoryError, DeleteRepositoryRequest,
    DescribeImagesRequest, DescribeRepositoriesError, DescribeRepositoriesRequest, Ecr, EcrClient,
    GetAuthorizationTokenRequest, ImageDetail, ImageIdentifier, PutLifecyclePolicyRequest, Repository, Tag,
    TagResourceRequest,
};
use rusoto_sts::{GetCallerIdentityRequest, Sts, StsClient};

use crate::build_platform::Image;
use crate::container_registry::errors::ContainerRegistryError;
use crate::container_registry::{ContainerRegistry, ContainerRegistryInfo, Kind};
use crate::events::{EngineEvent, EventMessage, InfrastructureStep, Stage};
use crate::io_models::context::Context;
use crate::logger::Logger;
use crate::runtime::block_on;
use retry::delay::Fixed;
use retry::Error::Operation;
use retry::{Error, OperationResult};
use serde_json::json;
use url::Url;
use uuid::Uuid;

pub struct ECR {
    context: Context,
    id: String,
    long_id: Uuid,
    name: String,
    access_key_id: String,
    secret_access_key: String,
    region: Region,
    registry_info: Option<ContainerRegistryInfo>,
    logger: Box<dyn Logger>,
    tags: HashMap<String, String>,
}

impl ECR {
    pub fn new(
        context: Context,
        id: &str,
        long_id: Uuid,
        name: &str,
        access_key_id: &str,
        secret_access_key: &str,
        region: &str,
        logger: Box<dyn Logger>,
        tags: HashMap<String, String>,
    ) -> Result<Self, ContainerRegistryError> {
        let mut cr = ECR {
            context,
            id: id.to_string(),
            long_id,
            name: name.to_string(),
            access_key_id: access_key_id.to_string(),
            secret_access_key: secret_access_key.to_string(),
            region: Region::from_str(region).unwrap(),
            registry_info: None,
            logger,
            tags,
        };

        let credentials = Self::get_credentials(&cr.ecr_client())?;
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
            self.get_event_details(Stage::Infrastructure(InfrastructureStep::ValidateSystemRequirements)),
            EventMessage::new_from_safe(msg),
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

    fn delete_repository(&self, repository_name: &str) -> Result<(), ContainerRegistryError> {
        let drr = DeleteRepositoryRequest {
            force: Some(true),
            registry_id: None,
            repository_name: repository_name.to_string(),
        };

        match block_on(self.ecr_client().delete_repository(drr)) {
            Ok(_) => Ok(()),
            Err(RusotoError::Service(DeleteRepositoryError::RepositoryNotFound(_))) => Ok(()),
            Err(err) => Err(ContainerRegistryError::CannotDeleteRepository {
                registry_name: self.registry_info().registry_name.clone(),
                repository_name: repository_name.to_string(),
                raw_error_message: err.to_string(),
            }),
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

    fn delete_image(&self, imge: &Image) -> Result<(), ContainerRegistryError> {
        let ret = block_on(self.ecr_client().batch_delete_image(BatchDeleteImageRequest {
            registry_id: None,
            repository_name: imge.repository_name.clone(),
            image_ids: vec![ImageIdentifier {
                image_digest: None,
                image_tag: Some(imge.tag.to_string()),
            }],
        }));

        match ret {
            Ok(_) => Ok(()),
            Err(e) => Err(ContainerRegistryError::CannotDeleteImage {
                registry_name: imge.registry_name.clone(),
                repository_name: imge.registry_name.clone(),
                image_name: imge.name(),
                raw_error_message: format!("{}", e),
            }),
        }
    }

    fn create_repository(
        &self,
        repository_name: &str,
        image_retention_time_in_seconds: u32,
    ) -> Result<Repository, ContainerRegistryError> {
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
                Ok(result) => OperationResult::Ok(result.repositories),
                Err(e) => match e {
                    RusotoError::Service(DescribeRepositoriesError::RepositoryNotFound(_)) => {
                        match block_on(self.ecr_client().create_repository(crr.clone())) {
                            // The Repo should be created at this point, but we want to verify that
                            // the describe/list return it now. we want to reloop so return a retry instead of a ok
                            Ok(_) => OperationResult::Retry(Err(ContainerRegistryError::CannotCreateRepository {
                                registry_name: self.name.to_string(),
                                repository_name: repository_name.to_string(),
                                raw_error_message: "Retry to check repository exist".to_string(),
                            })),
                            // Repo does not exist, so creating it
                            Err(err) => OperationResult::Retry(Err(ContainerRegistryError::CannotCreateRepository {
                                registry_name: self.name.to_string(),
                                repository_name: repository_name.to_string(),
                                raw_error_message: err.to_string(),
                            })),
                        }
                    }
                    // Unknown error, so retries ¯\_(ツ)_/¯
                    _ => OperationResult::Retry(Err(ContainerRegistryError::CannotCreateRepository {
                        registry_name: self.name.to_string(),
                        repository_name: repository_name.to_string(),
                        raw_error_message: e.to_string(),
                    })),
                },
            }
        });

        match repo_created {
            Err(Operation { error, .. }) => error,
            Err(Error::Internal(e)) => Err(ContainerRegistryError::CannotCreateRepository {
                registry_name: self.name.to_string(),
                repository_name: repository_name.to_string(),
                raw_error_message: e,
            }),
            Ok(repos) => {
                // apply retention policy
                if let Some(repos) = repos {
                    let retention_policy_in_days = match image_retention_time_in_seconds / 86400 {
                        0..=1 => 1,
                        _ => image_retention_time_in_seconds / 86400,
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
                    }?;

                    if let Some(repository_arn) = &repos[0].repository_arn {
                        let mut ecr_tags: Vec<Tag> = vec![];
                        for (key, value) in &self.tags {
                            ecr_tags.push(Tag {
                                key: Some(key.to_string()),
                                value: Some(value.to_string()),
                            })
                        }
                        let trr = TagResourceRequest {
                            resource_arn: repository_arn.to_string(),
                            tags: ecr_tags,
                        };

                        match block_on(self.ecr_client().tag_resource(trr)) {
                            Err(err) => Err(ContainerRegistryError::CannotSetRepositoryTags {
                                registry_name: self.name.to_string(),
                                repository_name: repository_name.to_string(),
                                raw_error_message: err.to_string(),
                            }),
                            _ => Ok(self.get_repository(repository_name).expect("cannot get repository")),
                        }?;
                    }

                    Ok(repos[0].clone())
                } else {
                    Err(ContainerRegistryError::Unknown {
                        raw_error_message: "Cannot get repositories".to_string(),
                    })
                }
            }
        }
    }

    fn get_or_create_repository(
        &self,
        repository_name: &str,
        image_retention_time_in_seconds: u32,
    ) -> Result<Repository, ContainerRegistryError> {
        self.log_info(format!("🗂️ Provisioning container repository {}", repository_name));

        // check if the repository already exists
        let repository = self.get_repository(repository_name);
        if let Some(repo) = repository {
            return Ok(repo);
        }

        self.create_repository(repository_name, image_retention_time_in_seconds)
    }

    pub fn get_credentials(ecr_client: &EcrClient) -> Result<ECRCredentials, ContainerRegistryError> {
        let r = block_on(ecr_client.get_authorization_token(GetAuthorizationTokenRequest::default()));

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

    fn long_id(&self) -> &Uuid {
        &self.long_id
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

    fn create_repository(
        &self,
        name: &str,
        image_retention_time_in_seconds: u32,
    ) -> Result<(), ContainerRegistryError> {
        let _ = self.get_or_create_repository(name, image_retention_time_in_seconds)?;
        Ok(())
    }

    fn delete_repository(&self, repository_name: &str) -> Result<(), ContainerRegistryError> {
        self.delete_repository(repository_name)
    }

    fn delete_image(&self, image: &Image) -> Result<(), ContainerRegistryError> {
        self.delete_image(image)
    }

    fn does_image_exists(&self, image: &Image) -> bool {
        self.get_image(image).is_some()
    }
}

pub struct ECRCredentials {
    pub access_token: String,
    pub password: String,
    pub endpoint_url: String,
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

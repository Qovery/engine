#![allow(clippy::field_reassign_with_default)]

use base64::Engine;
use base64::engine::general_purpose;
use std::collections::HashMap;
use std::time::Duration;

use rusoto_core::{Client, HttpClient, Region, RusotoError};
use rusoto_ecr::{
    BatchDeleteImageRequest, CreateRepositoryRequest, DeleteRepositoryError, DeleteRepositoryRequest,
    DescribeImagesRequest, DescribeRepositoriesError, DescribeRepositoriesRequest, Ecr, EcrClient,
    GetAuthorizationTokenRequest, ImageDetail, ImageIdentifier, ListTagsForResourceRequest, PutLifecyclePolicyRequest,
    Tag, TagResourceRequest,
};
use rusoto_sts::{GetCallerIdentityRequest, Sts, StsClient};

use super::RegistryTags;
use crate::events::{EngineEvent, EventMessage, InfrastructureStep, Stage};
use crate::infrastructure::models::build_platform::Image;
use crate::infrastructure::models::cloud_provider::aws::{AwsCredentials, new_rusoto_creds};
use crate::infrastructure::models::container_registry::errors::ContainerRegistryError;
use crate::infrastructure::models::container_registry::{
    ContainerRegistryInfo, InteractWithRegistry, Kind, Repository, RepositoryInfo,
    take_last_x_chars_and_remove_leading_dash_char,
};
use crate::io_models::context::Context;
use crate::logger::Logger;
use crate::runtime::block_on_with_timeout;
use retry::OperationResult;
use retry::delay::Fixed;
use serde_json::json;
use url::Url;
use uuid::Uuid;

pub struct ECR {
    context: Context,
    long_id: Uuid,
    name: String,
    credentials: AwsCredentials,
    region: Region,
    registry_info: Option<ContainerRegistryInfo>, // TODO(benjamin): code smell, should not come with an Option
    logger: Box<dyn Logger>,
    tags: HashMap<String, String>,
}

impl ECR {
    pub fn new(
        context: Context,
        long_id: Uuid,
        name: &str,
        credentials: AwsCredentials,
        region: Region,
        logger: Box<dyn Logger>,
        tags: HashMap<String, String>,
    ) -> Result<Self, ContainerRegistryError> {
        let mut cr = ECR {
            context,
            long_id,
            name: name.to_string(),
            credentials,
            region,
            registry_info: None,
            logger,
            tags,
        };

        let credentials = Self::get_credentials(&cr.ecr_client())?;
        let mut registry_url = Url::parse(credentials.endpoint_url.as_str()).unwrap();
        let _ = registry_url.set_username(&credentials.access_token);
        let _ = registry_url.set_password(Some(&credentials.password));

        cr.context
            .docker
            .login(&registry_url)
            .map_err(|_err| ContainerRegistryError::InvalidCredentials)?;
        const MAX_REGISTRY_NAME_LENGTH: usize = 118; // 128 (ECR limit) - 10 (prefix length)

        let registry_info = ContainerRegistryInfo {
            registry_name: cr.name.to_string(),
            get_registry_endpoint: Box::new(move |_registry_url_prefix| registry_url.clone()),
            get_registry_docker_json_config: Box::new(move |_docker_registry_info| None),
            insecure_registry: false,
            get_registry_url_prefix: Box::new(|_repository_name| None),
            get_shared_image_name: Box::new(|image_build_context| {
                let git_repo_truncated: String = take_last_x_chars_and_remove_leading_dash_char(
                    image_build_context.git_repo_url_sanitized.as_str(),
                    MAX_REGISTRY_NAME_LENGTH,
                );
                format!("{}-{}", image_build_context.cluster_id.short(), git_repo_truncated)
            }),
            get_image_name: Box::new(|img_name| img_name.to_string()),
            get_shared_repository_name: Box::new(|image_build_context| {
                let git_repo_truncated: String = take_last_x_chars_and_remove_leading_dash_char(
                    image_build_context.git_repo_url_sanitized.as_str(),
                    MAX_REGISTRY_NAME_LENGTH,
                );
                format!("{}-{}", image_build_context.cluster_id.short(), git_repo_truncated)
            }),
            get_repository_name: Box::new(|repository_name| repository_name.to_string()),
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

    pub fn client(&self) -> Client {
        Client::new_with(new_rusoto_creds(&self.credentials), HttpClient::new().unwrap())
    }

    pub fn ecr_client(&self) -> EcrClient {
        EcrClient::new_with_client(self.client(), self.region.clone())
    }

    fn delete_repository(&self, repository_name: &str) -> Result<(), ContainerRegistryError> {
        let drr = DeleteRepositoryRequest {
            force: Some(true),
            registry_id: None,
            repository_name: repository_name.to_string(),
        };

        match block_on_with_timeout(self.ecr_client().delete_repository(drr)) {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(RusotoError::Service(DeleteRepositoryError::RepositoryNotFound(_)))) => Ok(()),
            Ok(Err(err)) => Err(ContainerRegistryError::CannotDeleteRepository {
                registry_name: self.registry_info().registry_name.clone(),
                repository_name: repository_name.to_string(),
                raw_error_message: err.to_string(),
            }),
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

        let r = block_on_with_timeout(self.ecr_client().describe_images(dir));

        match r {
            Err(_) | Ok(Err(_)) => None,
            Ok(Ok(res)) => match res.image_details {
                // assume there is only one repository returned - why? Because we set only one repository_names above
                Some(image_details) => image_details.into_iter().next(),
                _ => None,
            },
        }
    }

    fn delete_image(&self, image: &Image) -> Result<(), ContainerRegistryError> {
        let request = BatchDeleteImageRequest {
            registry_id: None,
            repository_name: image.repository_name().to_string(),
            image_ids: vec![ImageIdentifier {
                image_digest: None,
                image_tag: Some(image.tag.to_string()),
            }],
        };

        match block_on_with_timeout(self.ecr_client().batch_delete_image(request)) {
            Ok(_) => Ok(()),
            Err(e) => Err(ContainerRegistryError::CannotDeleteImage {
                registry_name: image.registry_name.clone(),
                repository_name: image.registry_name.clone(),
                image_name: image.name(),
                raw_error_message: format!("{e}"),
            }),
        }
    }

    fn create_repository(
        &self,
        repository_name: &str,
        image_retention_time_in_seconds: u32,
        registry_tags: RegistryTags,
    ) -> Result<Repository, ContainerRegistryError> {
        let container_registry_request = DescribeRepositoriesRequest {
            repository_names: Some(vec![repository_name.to_string()]),
            ..Default::default()
        };

        let mut tags = vec![];
        if let Some(cluster_id) = registry_tags.cluster_id {
            tags.push(Tag {
                key: Some("ClusterId".to_string()),
                value: Some(cluster_id.to_string()),
            })
        };
        if let Some(environment_id) = registry_tags.environment_id {
            tags.push(Tag {
                key: Some("EnvironmentId".to_string()),
                value: Some(environment_id.to_string()),
            })
        };
        if let Some(project_id) = registry_tags.project_id {
            tags.push(Tag {
                key: Some("ProjectId".to_string()),
                value: Some(project_id.to_string()),
            })
        };
        if let Some(duration) = registry_tags.resource_ttl {
            tags.push(Tag {
                key: Some("ttl".to_string()),
                value: Some(duration.as_secs().to_string()),
            })
        };
        let crr = CreateRepositoryRequest {
            repository_name: repository_name.to_string(),
            tags: Some(tags),
            ..Default::default()
        };

        // ensure repository is created
        // need to do all this checks and retry because of several issues encountered like: 200 API response code while repo is not created
        let repo_created = retry::retry(Fixed::from_millis(5000).take(24), || {
            info!("Trying to create ECR repository {}", repository_name);
            let repositories = block_on_with_timeout(
                self.ecr_client()
                    .describe_repositories(container_registry_request.clone()),
            );
            match repositories.unwrap_or(Err(RusotoError::Blocking)) {
                // Repo already exist, so ok
                Ok(result) => OperationResult::Ok(result.repositories),
                Err(e) => match e {
                    RusotoError::Service(DescribeRepositoriesError::RepositoryNotFound(_)) => {
                        match block_on_with_timeout(self.ecr_client().create_repository(crr.clone())) {
                            // The Repo should be created at this point, but we want to verify that the describe/list return it now.
                            // So we reloop in order to be sure it is available when we do a describe
                            Ok(_) => OperationResult::Retry(e),
                            // Should not happen
                            Err(err) => {
                                error!("Error while wanting to create ECR repository {:?}", err);
                                OperationResult::Retry(e)
                            }
                        }
                    }

                    // Unknown error, so retries ¯\_(ツ)_/¯
                    err => OperationResult::Retry(err),
                },
            }
        });

        match repo_created {
            Err(err) => {
                error!("Cannot create AWS repository due to {:?}", err.error);
                Err(ContainerRegistryError::CannotCreateRepository {
                    registry_name: self.name.to_string(),
                    repository_name: repository_name.to_string(),
                    raw_error_message: err.error.to_string(),
                })
            }
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

                    match block_on_with_timeout(self.ecr_client().put_lifecycle_policy(plp)) {
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

                        match block_on_with_timeout(self.ecr_client().tag_resource(trr)) {
                            Err(err) => Err(ContainerRegistryError::CannotSetRepositoryTags {
                                registry_name: self.name.to_string(),
                                repository_name: repository_name.to_string(),
                                raw_error_message: err.to_string(),
                            }),
                            _ => Ok(self.get_repository(repository_name).expect("cannot get repository")),
                        }?;
                    }

                    // return the created repo via get
                    self.get_repository(repository_name) // TODO(benjaminch): maybe extra get is avoidable here?
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
        registry_tags: RegistryTags,
    ) -> Result<(Repository, RepositoryInfo), ContainerRegistryError> {
        // check if the repository already exists
        if let Ok(repository) = self.get_repository(repository_name) {
            return Ok((repository, RepositoryInfo { created: false }));
        }

        // create it if it doesn't exist
        match self.create_repository(repository_name, image_retention_time_in_seconds, registry_tags) {
            Ok(repository) => Ok((repository, RepositoryInfo { created: true })),
            Err(e) => Err(e),
        }
    }

    pub fn get_credentials(ecr_client: &EcrClient) -> Result<ECRCredentials, ContainerRegistryError> {
        let r = block_on_with_timeout(ecr_client.get_authorization_token(GetAuthorizationTokenRequest::default()));

        let (access_token, password, endpoint_url) = match r {
            Ok(Ok(t)) => match t.authorization_data {
                Some(authorization_data) => {
                    let ad = authorization_data.first().unwrap();
                    let b64_token = ad.authorization_token.as_ref().unwrap();

                    let decoded_token = general_purpose::STANDARD.decode(b64_token).unwrap();
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
        let s = block_on_with_timeout(client.get_caller_identity(GetCallerIdentityRequest::default()));

        match s {
            Ok(_) => Ok(()),
            Err(_) => Err(ContainerRegistryError::InvalidCredentials),
        }
    }
}

impl InteractWithRegistry for ECR {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::Ecr
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

    fn get_registry_endpoint(&self, registry_endpoint_prefix: Option<&str>) -> Url {
        self.registry_info().get_registry_endpoint(registry_endpoint_prefix)
    }

    fn create_repository(
        &self,
        _registry_name: Option<&str>,
        name: &str,
        image_retention_time_in_seconds: u32,
        registry_tags: RegistryTags,
    ) -> Result<(Repository, RepositoryInfo), ContainerRegistryError> {
        let repository_info = self.get_or_create_repository(name, image_retention_time_in_seconds, registry_tags)?;
        Ok(repository_info)
    }

    fn get_repository(&self, repository_name: &str) -> Result<Repository, ContainerRegistryError> {
        let ecr_client = self.ecr_client();
        let mut drr = DescribeRepositoriesRequest::default();
        drr.repository_names = Some(vec![repository_name.to_string()]);

        match block_on_with_timeout(ecr_client.describe_repositories(drr)) {
            Err(e) => Err(ContainerRegistryError::CannotGetRepository {
                registry_name: match &self.registry_info {
                    Some(i) => i.registry_name.to_string(),
                    None => "".to_string(),
                },
                repository_name: repository_name.to_string(),
                raw_error_message: e.to_string(),
            }),
            Ok(Err(e)) => Err(ContainerRegistryError::CannotGetRepository {
                registry_name: match &self.registry_info {
                    Some(i) => i.registry_name.to_string(),
                    None => "".to_string(),
                },
                repository_name: repository_name.to_string(),
                raw_error_message: e.to_string(),
            }),
            Ok(Ok(res)) => match res.repositories {
                // assume there is only one repository returned - why? Because we set only one repository_names above
                Some(repositories) => {
                    match repositories.into_iter().next() {
                        Some(r) => {
                            // get tags for repository
                            let tags = match block_on_with_timeout(ecr_client.list_tags_for_resource(
                                ListTagsForResourceRequest {
                                    resource_arn: r.repository_arn.unwrap_or("".to_string()),
                                },
                            )) {
                                Ok(Ok(tags_res)) => tags_res.tags,
                                _ => None,
                            };

                            let mut ttl = None;
                            for tag in tags.clone().unwrap_or_default() {
                                if let (Some(k), Some(v)) = (&tag.key, &tag.value) {
                                    if k.as_str() == "ttl" {
                                        if let Ok(d) = v.parse() {
                                            ttl = Some(Duration::from_secs(d));
                                            break;
                                        }
                                    }
                                }
                            }

                            let created_repository_name = r.repository_name.unwrap_or_default();

                            Ok(Repository {
                                registry_id: r.registry_id.unwrap_or("".to_string()),
                                name: created_repository_name.to_string(),
                                uri: r.repository_uri,
                                ttl,
                                labels: tags.map(|t| {
                                    t.into_iter()
                                        .map(|i| (i.key.unwrap_or_default(), i.value.unwrap_or_default()))
                                        .collect()
                                }),
                            })
                        }
                        None => Err(ContainerRegistryError::CannotGetRepository {
                            registry_name: match &self.registry_info {
                                Some(i) => i.registry_name.to_string(),
                                None => "".to_string(),
                            },
                            repository_name: repository_name.to_string(),
                            raw_error_message: format!("No repository found with name `{}`", repository_name),
                        }),
                    }
                }
                _ => Err(ContainerRegistryError::CannotGetRepository {
                    registry_name: match &self.registry_info {
                        Some(i) => i.registry_name.to_string(),
                        None => "".to_string(),
                    },
                    repository_name: repository_name.to_string(),
                    raw_error_message: format!("No repository found with name `{}`", repository_name),
                }),
            },
        }
    }

    fn delete_repository(&self, repository_name: &str) -> Result<(), ContainerRegistryError> {
        self.delete_repository(repository_name)
    }

    fn delete_image(&self, image: &Image) -> Result<(), ContainerRegistryError> {
        self.delete_image(image)
    }

    fn image_exists(&self, image: &Image) -> bool {
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

use std::borrow::Borrow;
use std::str::FromStr;

use rusoto_core::{Client, HttpClient, Region, RusotoError};
use rusoto_credential::StaticProvider;
use rusoto_ecr::{
    CreateRepositoryRequest, DescribeImagesRequest, DescribeRepositoriesError, DescribeRepositoriesRequest, Ecr,
    EcrClient, GetAuthorizationTokenRequest, ImageDetail, ImageIdentifier, PutLifecyclePolicyRequest, Repository,
};
use rusoto_sts::{GetCallerIdentityRequest, Sts, StsClient};

use crate::build_platform::Image;
use crate::cmd::docker::to_engine_error;
use crate::container_registry::{ContainerRegistry, ContainerRegistryInfo, Kind};
use crate::errors::{CommandError, EngineError};
use crate::events::{EngineEvent, EventMessage, ToTransmitter, Transmitter};
use crate::logger::{LogLevel, Logger};
use crate::models::{Context, Listen, Listener, Listeners};
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
        logger: Box<dyn Logger>,
    ) -> Result<Self, EngineError> {
        let mut cr = ECR {
            context,
            id: id.to_string(),
            name: name.to_string(),
            access_key_id: access_key_id.to_string(),
            secret_access_key: secret_access_key.to_string(),
            region: Region::from_str(region).unwrap(),
            registry_info: None,
            listeners: vec![],
            logger,
        };

        let credentials = cr.get_credentials()?;
        let mut registry_url = Url::parse(credentials.endpoint_url.as_str()).unwrap();
        let _ = registry_url.set_username(&credentials.access_token);
        let _ = registry_url.set_password(Some(&credentials.password));

        let _ = cr
            .context
            .docker
            .login(&registry_url)
            .map_err(|err| to_engine_error(&cr.get_event_details(), err))?;

        let registry_info = ContainerRegistryInfo {
            endpoint: registry_url,
            registry_name: cr.name.to_string(),
            registry_docker_json_config: None,
            get_image_name: Box::new(|img_name| img_name.to_string()),
        };

        cr.registry_info = Some(registry_info);
        Ok(cr)
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
        dir.repository_name = image.name().to_string();

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

    fn create_repository(&self, repository_name: &str) -> Result<Repository, EngineError> {
        let event_details = self.get_event_details();
        self.logger().log(
            LogLevel::Info,
            EngineEvent::Info(
                event_details.clone(),
                EventMessage::new_from_safe(format!("Creating ECR repository {}", &repository_name)),
            ),
        );

        let mut repo_creation_counter = 0;
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
            match block_on(
                self.ecr_client()
                    .describe_repositories(container_registry_request.clone()),
            ) {
                Ok(x) => {
                    self.logger().log(
                        LogLevel::Debug,
                        EngineEvent::Debug(
                            event_details.clone(),
                            EventMessage::new_from_safe(format!("Created {:?} repository", x)),
                        ),
                    );
                    OperationResult::Ok(())
                }
                Err(e) => {
                    match e {
                        RusotoError::Service(s) => match s {
                            DescribeRepositoriesError::RepositoryNotFound(_) => {
                                if repo_creation_counter != 0 {
                                    self.logger().log(
                                        LogLevel::Warning,
                                        EngineEvent::Warning(
                                            event_details.clone(),
                                            EventMessage::new_from_safe(format!(
                                                "Repository {} was not found, {}x retrying...",
                                                &repository_name, &repo_creation_counter
                                            )),
                                        ),
                                    );
                                }
                                repo_creation_counter += 1;
                            }
                            _ => self.logger().log(
                                LogLevel::Warning,
                                EngineEvent::Warning(
                                    event_details.clone(),
                                    EventMessage::new(
                                        "Error while trying to create repository.".to_string(),
                                        Some(format!("{:?}", s)),
                                    ),
                                ),
                            ),
                        },
                        _ => self.logger().log(
                            LogLevel::Warning,
                            EngineEvent::Warning(
                                event_details.clone(),
                                EventMessage::new(
                                    "Error while trying to create repository.".to_string(),
                                    Some(format!("{:?}", e)),
                                ),
                            ),
                        ),
                    }

                    // TODO: This behavior is weird, returning an ok message saying repository has been created in an error ...
                    let msg = match block_on(self.ecr_client().create_repository(crr.clone())) {
                        Ok(_) => format!("repository {} created", &repository_name),
                        Err(err) => format!("{:?}", err),
                    };

                    OperationResult::Retry(Err(EngineError::new_container_registry_namespace_creation_error(
                        event_details.clone(),
                        repository_name.to_string(),
                        self.name_with_id(),
                        CommandError::new(msg.to_string(), Some("Can't create ECR repository".to_string())),
                    )))
                }
            }
        });

        match repo_created {
            Ok(_) => self.logger.log(
                LogLevel::Info,
                EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe(format!(
                        "repository {} created after {} attempt(s)",
                        &repository_name, repo_creation_counter,
                    )),
                ),
            ),
            Err(Operation { error, .. }) => return error,
            Err(retry::Error::Internal(e)) => {
                return Err(EngineError::new_container_registry_namespace_creation_error(
                    event_details.clone(),
                    repository_name.to_string(),
                    self.name_with_id(),
                    CommandError::new_from_safe_message(e),
                ))
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
            Err(err) => Err(
                EngineError::new_container_registry_repository_set_lifecycle_policy_error(
                    event_details.clone(),
                    repository_name.to_string(),
                    CommandError::new_from_safe_message(err.to_string()),
                ),
            ),
            _ => Ok(self.get_repository(repository_name).expect("cannot get repository")),
        }
    }

    fn get_or_create_repository(&self, repository_name: &str) -> Result<Repository, EngineError> {
        let event_details = self.get_event_details();

        // check if the repository already exists
        let repository = self.get_repository(repository_name);
        if repository.is_some() {
            self.logger.log(
                LogLevel::Info,
                EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe(format!("ECR repository {} already exists", repository_name)),
                ),
            );
            return Ok(repository.unwrap());
        }

        self.create_repository(repository_name)
    }

    fn get_credentials(&self) -> Result<ECRCredentials, EngineError> {
        let event_details = self.get_event_details();
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
                    return Err(EngineError::new_container_registry_get_credentials_error(
                        event_details.clone(),
                        self.name_with_id(),
                    ));
                }
            },
            _ => {
                return Err(EngineError::new_container_registry_get_credentials_error(
                    event_details.clone(),
                    self.name_with_id(),
                ));
            }
        };

        Ok(ECRCredentials::new(access_token, password, endpoint_url))
    }
}

impl ToTransmitter for ECR {
    fn to_transmitter(&self) -> Transmitter {
        Transmitter::ContainerRegistry(self.id().to_string(), self.name().to_string())
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

    fn is_valid(&self) -> Result<(), EngineError> {
        let client = StsClient::new_with_client(self.client(), Region::default());
        let s = block_on(client.get_caller_identity(GetCallerIdentityRequest::default()));

        match s {
            Ok(_) => Ok(()),
            Err(_) => Err(EngineError::new_client_invalid_cloud_provider_credentials(
                self.get_event_details(),
            )),
        }
    }

    fn registry_info(&self) -> &ContainerRegistryInfo {
        // At this point the registry info should be initialize, so unwrap is safe
        self.registry_info.as_ref().unwrap()
    }

    fn create_registry(&self) -> Result<(), EngineError> {
        // Nothing to do, ECR require to create only repository
        Ok(())
    }

    fn create_repository(&self, name: &str) -> Result<(), EngineError> {
        let _ = self.get_or_create_repository(name)?;
        Ok(())
    }

    fn does_image_exists(&self, image: &Image) -> bool {
        self.get_image(image).is_some()
    }

    fn logger(&self) -> &dyn Logger {
        self.logger.borrow()
    }
}

impl Listen for ECR {
    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
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

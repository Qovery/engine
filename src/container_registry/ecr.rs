use std::rc::Rc;
use std::str::FromStr;

use rusoto_core::{Client, HttpClient, Region, RusotoError};
use rusoto_credential::StaticProvider;
use rusoto_ecr::{
    CreateRepositoryError, CreateRepositoryRequest, DescribeImagesRequest,
    DescribeRepositoriesRequest, Ecr, EcrClient, GetAuthorizationTokenRequest, ImageDetail,
    ImageIdentifier, PutLifecyclePolicyRequest, Repository,
};
use rusoto_sts::{GetCallerIdentityRequest, Sts, StsClient};

use crate::build_platform::Image;
use crate::cmd;
use crate::container_registry::{ContainerRegistry, Kind, PushResult};
use crate::error::{EngineError, EngineErrorCause};
use crate::models::{
    Context, Listener, Listeners, ListenersHelper, ProgressInfo, ProgressLevel, ProgressListener,
    ProgressScope,
};
use crate::runtime::async_run;

pub struct ECR {
    context: Context,
    id: String,
    name: String,
    access_key_id: String,
    secret_access_key: String,
    region: Region,
    listeners: Listeners,
}

impl ECR {
    pub fn new(
        context: Context,
        id: &str,
        name: &str,
        access_key_id: &str,
        secret_access_key: &str,
        region: &str,
    ) -> Self {
        ECR {
            context,
            id: id.to_string(),
            name: name.to_string(),
            access_key_id: access_key_id.to_string(),
            secret_access_key: secret_access_key.to_string(),
            region: Region::from_str(region).unwrap(),
            listeners: vec![],
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

    fn get_repository(&self, image: &Image) -> Option<Repository> {
        let mut drr = DescribeRepositoriesRequest::default();
        drr.repository_names = Some(vec![image.name.to_string()]);

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

    fn get_image(&self, image: &Image) -> Option<ImageDetail> {
        let mut dir = DescribeImagesRequest::default();
        dir.repository_name = image.name.to_string();

        let mut image_identifier = ImageIdentifier::default();
        image_identifier.image_tag = Some(image.tag.to_string());
        dir.image_ids = Some(vec![image_identifier]);

        let r = async_run(self.ecr_client().describe_images(dir));

        match r {
            Err(_) => None,
            Ok(res) => match res.image_details {
                // assume there is only one repository returned - why? Because we set only one repository_names above
                Some(image_details) => image_details.into_iter().next(),
                _ => None,
            },
        }
    }

    fn docker_envs(&self) -> Vec<(&str, &str)> {
        match self.context.docker_tcp_socket() {
            Some(tcp_socket) => vec![("DOCKER_HOST", tcp_socket.as_str())],
            None => vec![],
        }
    }

    fn push_image(&self, dest: String, image: &Image) -> Result<PushResult, EngineError> {
        // READ https://docs.aws.amazon.com/AmazonECR/latest/userguide/docker-push-ecr-image.html
        // docker tag e9ae3c220b23 aws_account_id.dkr.ecr.region.amazonaws.com/my-web-app

        match cmd::utilities::exec_with_envs(
            "docker",
            vec!["tag", image.name_with_tag().as_str(), dest.as_str()],
            self.docker_envs(),
        ) {
            Err(_) => {
                return Err(self.engine_error(
                    EngineErrorCause::Internal,
                    format!(
                        "failed to tag image ({}) {:?}",
                        image.name_with_tag(),
                        image,
                    ),
                ));
            }
            _ => {}
        };

        // docker push aws_account_id.dkr.ecr.region.amazonaws.com/my-web-app
        match cmd::utilities::exec_with_envs(
            "docker",
            vec!["push", dest.as_str()],
            self.docker_envs(),
        ) {
            Err(_) => {
                return Err(self.engine_error(
                    EngineErrorCause::Internal,
                    format!(
                        "failed to push image {:?} into ECR {}",
                        image,
                        self.name_with_id(),
                    ),
                ));
            }
            _ => {}
        };

        let mut image = image.clone();
        image.registry_url = Some(dest);

        Ok(PushResult { image })
    }

    fn create_repository(&self, image: &Image) -> Result<Repository, EngineError> {
        info!("ECR create repository {}", image.name.as_str());
        let mut crr = CreateRepositoryRequest::default();
        crr.repository_name = image.name.clone();

        let r = async_run(self.ecr_client().create_repository(crr));
        match r {
            Err(err) => match err {
                RusotoError::Service(ref err) => info!("{:?}", err),
                _ => {
                    return Err(self.engine_error(
                        EngineErrorCause::Internal,
                        format!(
                            "can't create ECR repository {} for {}",
                            image.name.as_str(),
                            self.name_with_id()
                        ),
                    ));
                }
            },
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
                "countNumber": 365,
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
            Err(_) => Err(self.engine_error(
                EngineErrorCause::Internal,
                format!(
                    "can't set lifecycle policy to ECR repository {} for {}",
                    image.name.as_str(),
                    self.name_with_id()
                ),
            )),
            _ => Ok(self.get_repository(&image).unwrap()),
        }
    }

    fn get_or_create_repository(&self, image: &Image) -> Result<Repository, EngineError> {
        // check if the repository already exists
        let repository = self.get_repository(&image);
        if repository.is_some() {
            info!("ECR repository {} already exists", image.name.as_str());
            return Ok(repository.unwrap());
        }

        self.create_repository(&image)
    }
}

impl ContainerRegistry for ECR {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::ECR
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn is_valid(&self) -> Result<(), EngineError> {
        let client = StsClient::new_with_client(self.client(), Region::default());
        let s = async_run(client.get_caller_identity(GetCallerIdentityRequest::default()));

        match s {
            Ok(_) => Ok(()),
            Err(_) => Err(self.engine_error(
                EngineErrorCause::User(
                    "Your ECR account seems to be no longer valid (bad Credentials). \
                    Please contact your Organization administrator to fix or change the Credentials.",
                ),
                format!("bad ECR credentials for {}", self.name_with_id()),
            )),
        }
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }

    fn on_create(&self) -> Result<(), EngineError> {
        info!("ECR.on_create() called");
        Ok(())
    }

    fn on_create_error(&self) -> Result<(), EngineError> {
        unimplemented!()
    }

    fn on_delete(&self) -> Result<(), EngineError> {
        unimplemented!()
    }

    fn on_delete_error(&self) -> Result<(), EngineError> {
        unimplemented!()
    }

    fn does_image_exists(&self, image: &Image) -> bool {
        self.get_repository(image).is_some()
    }

    fn push(&self, image: &Image, force_push: bool) -> Result<PushResult, EngineError> {
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
                None => {
                    return Err(self.engine_error(
                        EngineErrorCause::Internal,
                        format!(
                            "failed to retrieve credentials and endpoint URL from ECR {}",
                            self.name_with_id(),
                        ),
                    ));
                }
            },
            _ => {
                return Err(self.engine_error(
                    EngineErrorCause::Internal,
                    format!(
                        "failed to retrieve credentials and endpoint URL from ECR {}",
                        self.name_with_id(),
                    ),
                ));
            }
        };

        let repository = match if force_push {
            self.create_repository(image)
        } else {
            self.get_or_create_repository(image)
        } {
            Ok(r) => r,
            _ => {
                return Err(self.engine_error(
                    EngineErrorCause::Internal,
                    format!(
                        "failed to create ECR repository for {} with image {:?}",
                        self.name_with_id(),
                        image,
                    ),
                ));
            }
        };

        match cmd::utilities::exec_with_envs(
            "docker",
            vec![
                "login",
                "-u",
                access_token.as_str(),
                "-p",
                password.as_str(),
                endpoint_url.as_str(),
            ],
            self.docker_envs(),
        ) {
            Err(_) => return Err(
                self.engine_error(
                    EngineErrorCause::User("Your ECR account seems to be no longer valid (bad Credentials). \
                    Please contact your Organization administrator to fix or change the Credentials."),
                    format!("failed to login to ECR {}", self.name_with_id()))
            ),
            _ => {}
        };

        let dest = format!(
            "{}:{}",
            repository.repository_uri.unwrap(),
            image.tag.as_str()
        );

        let listeners_helper = ListenersHelper::new(&self.listeners);

        if !force_push && self.get_image(image).is_some() {
            // check if image does exist - if yes, do not upload it again
            let info_message = format!(
                "image {:?} does already exist into ECR {} repository - no need to upload it",
                image,
                self.name()
            );

            info!("{}", info_message.as_str());

            listeners_helper.start_in_progress(ProgressInfo::new(
                ProgressScope::Application {
                    id: image.application_id.clone(),
                },
                ProgressLevel::Info,
                Some(info_message),
                self.context.execution_id(),
            ));

            let mut image = image.clone();
            image.registry_url = Some(dest);

            return Ok(PushResult { image });
        }

        let info_message = format!(
            "image {:?} does not exist into ECR {} repository - let's upload it",
            image,
            self.name()
        );

        info!("{}", info_message.as_str());

        listeners_helper.start_in_progress(ProgressInfo::new(
            ProgressScope::Application {
                id: image.application_id.clone(),
            },
            ProgressLevel::Info,
            Some(info_message),
            self.context.execution_id(),
        ));

        self.push_image(dest, image)
    }

    fn push_error(&self, image: &Image) -> Result<PushResult, EngineError> {
        // TODO change this
        Ok(PushResult {
            image: image.clone(),
        })
    }
}

extern crate reqwest;

use reqwest::StatusCode;
use std::borrow::Borrow;

use crate::build_platform::Image;
use crate::cmd::command::QoveryCommand;
use crate::container_registry::docker::{docker_pull_image, docker_tag_and_push_image};
use crate::container_registry::{ContainerRegistry, Kind, PullResult, PushResult};
use crate::errors::{CommandError, EngineError};
use crate::events::{EngineEvent, EventMessage, ToTransmitter, Transmitter};
use crate::logger::{LogLevel, Logger};
use crate::models::{
    Context, Listen, Listener, Listeners, ListenersHelper, ProgressInfo, ProgressLevel, ProgressScope,
};

pub struct DockerHub {
    context: Context,
    id: String,
    name: String,
    login: String,
    password: String,
    listeners: Listeners,
    logger: Box<dyn Logger>,
}

impl DockerHub {
    pub fn new(context: Context, id: &str, name: &str, login: &str, password: &str, logger: Box<dyn Logger>) -> Self {
        DockerHub {
            context,
            id: id.to_string(),
            name: name.to_string(),
            login: login.to_string(),
            password: password.to_string(),
            listeners: vec![],
            logger,
        }
    }

    pub fn exec_docker_login(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details();

        let envs = match self.context.docker_tcp_socket() {
            Some(tcp_socket) => vec![("DOCKER_HOST", tcp_socket.as_str())],
            None => vec![],
        };

        let mut cmd = QoveryCommand::new(
            "docker",
            &vec!["login", "-u", self.login.as_str(), "-p", self.password.as_str()],
            &envs,
        );

        match cmd.exec() {
            Ok(_) => Ok(()),
            Err(_) => Err(EngineError::new_client_invalid_cloud_provider_credentials(
                event_details,
            )),
        }
    }

    fn pull_image(&self, dest: String, image: &Image) -> Result<PullResult, EngineError> {
        let event_details = self.get_event_details();
        match docker_pull_image(self.kind(), vec![], dest.clone(), event_details.clone(), self.logger()) {
            Ok(_) => {
                let mut image = image.clone();
                image.registry_url = Some(dest);
                Ok(PullResult::Some(image))
            }
            Err(e) => Err(EngineError::new_docker_pull_image_error(
                event_details,
                image.name.to_string(),
                dest.to_string(),
                e,
            )),
        }
    }
}

impl ToTransmitter for DockerHub {
    fn to_transmitter(&self) -> Transmitter {
        Transmitter::ContainerRegistry(self.id().to_string(), self.name().to_string())
    }
}

impl ContainerRegistry for DockerHub {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::DockerHub
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn is_valid(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_create(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_create_error(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_delete(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn on_delete_error(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn does_image_exists(&self, image: &Image) -> bool {
        let event_details = self.get_event_details();
        use reqwest::blocking::Client;
        let client = Client::new();
        let path = format!(
            "https://index.docker.io/v1/repositories/{}/{}/tags",
            &self.login, image.name
        );
        let res = client
            .get(path.as_str())
            .basic_auth(&self.login, Option::from(&self.password))
            .send();

        // TODO (mzo) no check of existing tags as in others impl ?
        match res {
            Ok(out) => matches!(out.status(), StatusCode::OK),
            Err(e) => {
                self.logger.log(
                    LogLevel::Error,
                    EngineEvent::Error(
                        EngineError::new_container_registry_repository_doesnt_exist(
                            event_details.clone(),
                            image.name.to_string(),
                            Some(CommandError::new(
                                e.to_string(),
                                Some("Error while trying to retrieve if DockerHub repository exist.".to_string()),
                            )),
                        ),
                        None,
                    ),
                );
                false
            }
        }
    }

    fn pull(&self, image: &Image) -> Result<PullResult, EngineError> {
        let event_details = self.get_event_details();
        let listeners_helper = ListenersHelper::new(&self.listeners);

        if !self.does_image_exists(image) {
            let info_message = format!(
                "image {:?} does not exist in DockerHub {} repository",
                image,
                self.name()
            );

            self.logger.log(
                LogLevel::Info,
                EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe(info_message.to_string()),
                ),
            );

            listeners_helper.deployment_in_progress(ProgressInfo::new(
                ProgressScope::Application {
                    id: image.application_id.clone(),
                },
                ProgressLevel::Info,
                Some(info_message),
                self.context.execution_id(),
            ));

            return Ok(PullResult::None);
        }

        let info_message = format!("pull image {:?} from DockerHub {} repository", image, self.name());

        self.logger.log(
            LogLevel::Info,
            EngineEvent::Info(
                event_details.clone(),
                EventMessage::new_from_safe(info_message.to_string()),
            ),
        );

        listeners_helper.deployment_in_progress(ProgressInfo::new(
            ProgressScope::Application {
                id: image.application_id.clone(),
            },
            ProgressLevel::Info,
            Some(info_message),
            self.context.execution_id(),
        ));

        let _ = self.exec_docker_login()?;

        let dest = format!("{}/{}", self.login.as_str(), image.name_with_tag().as_str());

        // pull image
        self.pull_image(dest, image)
    }

    fn push(&self, image: &Image, force_push: bool) -> Result<PushResult, EngineError> {
        let event_details = self.get_event_details();

        let _ = self.exec_docker_login()?;

        let dest = format!("{}/{}", self.login.as_str(), image.name_with_tag().as_str());
        let listeners_helper = ListenersHelper::new(&self.listeners);

        if !force_push && self.does_image_exists(image) {
            // check if image does exist - if yes, do not upload it again
            let info_message = format!(
                "image {:?} found on DockerHub {} repository, container build is not required",
                image,
                self.name()
            );

            self.logger.log(
                LogLevel::Info,
                EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe(info_message.to_string()),
                ),
            );

            listeners_helper.deployment_in_progress(ProgressInfo::new(
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
            "image {:?} does not exist on DockerHub {} repository, starting image upload",
            image,
            self.name()
        );

        self.logger.log(
            LogLevel::Info,
            EngineEvent::Info(
                event_details.clone(),
                EventMessage::new_from_safe(info_message.to_string()),
            ),
        );

        listeners_helper.deployment_in_progress(ProgressInfo::new(
            ProgressScope::Application {
                id: image.application_id.clone(),
            },
            ProgressLevel::Info,
            Some(info_message),
            self.context.execution_id(),
        ));

        let dest_latest_tag = format!("{}/{}:latest", self.login.as_str(), image.name);
        match docker_tag_and_push_image(
            self.kind(),
            vec![],
            &image,
            dest.clone(),
            dest_latest_tag,
            event_details.clone(),
            self.logger(),
        ) {
            Ok(_) => {
                let mut image = image.clone();
                image.registry_url = Some(dest);
                Ok(PushResult { image })
            }
            Err(e) => Err(EngineError::new_docker_push_image_error(
                event_details.clone(),
                image.name.to_string(),
                dest.to_string(),
                e,
            )),
        }
    }

    fn push_error(&self, _image: &Image) -> Result<PushResult, EngineError> {
        unimplemented!()
    }

    fn logger(&self) -> &dyn Logger {
        self.logger.borrow()
    }
}

impl Listen for DockerHub {
    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }
}

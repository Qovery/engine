extern crate reqwest;

use reqwest::StatusCode;

use crate::build_platform::Image;
use crate::cmd;
use crate::container_registry::{ContainerRegistry, EngineError, Kind, PushResult};
use crate::error::EngineErrorCause;
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
}

impl DockerHub {
    pub fn new(context: Context, id: &str, name: &str, login: &str, password: &str) -> Self {
        DockerHub {
            context,
            id: id.to_string(),
            name: name.to_string(),
            login: login.to_string(),
            password: password.to_string(),
            listeners: vec![],
        }
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
        // check the version of docker and print it as info
        let mut output_from_cmd = String::new();
        let _ = cmd::utilities::exec_with_output(
            "docker",
            vec!["--version"],
            |r_out| match r_out {
                Ok(s) => output_from_cmd.push_str(&s.to_owned()),
                Err(e) => error!("Error while getting sdtout from docker {}", e),
            },
            |r_err| match r_err {
                Ok(s) => error!("Error executing docker command {}", s),
                Err(e) => error!("Error while getting stderr from docker {}", e),
            },
        );

        info!("Using Docker: {}", output_from_cmd);
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

        match res {
            Ok(out) => match out.status() {
                StatusCode::OK => true,
                _ => false,
            },
            Err(e) => {
                error!("While trying to retrieve if DockerHub repository exist {:?}", e);
                false
            }
        }
    }

    fn push(&self, image: &Image, force_push: bool) -> Result<PushResult, EngineError> {
        let envs = match self.context.docker_tcp_socket() {
            Some(tcp_socket) => vec![("DOCKER_HOST", tcp_socket.as_str())],
            None => vec![],
        };

        match cmd::utilities::exec_with_envs(
            "docker",
            vec!["login", "-u", self.login.as_str(), "-p", self.password.as_str()],
            envs.clone(),
        ) {
            Err(_) => {
                return Err(self.engine_error(
                    EngineErrorCause::User(
                        "Your DockerHub account seems to be no longer valid (bad Credentials). \
                    Please contact your Organization administrator to fix or change the Credentials.",
                    ),
                    format!("failed to login to DockerHub {}", self.name_with_id()),
                ));
            }
            _ => {}
        };

        let dest = format!("{}/{}", self.login.as_str(), image.name_with_tag().as_str());
        let listeners_helper = ListenersHelper::new(&self.listeners);

        if !force_push && self.does_image_exists(image) {
            // check if image does exist - if yes, do not upload it again
            let info_message = format!(
                "image {:?} found on DockerHub {} repository, container build is not required",
                image,
                self.name()
            );

            info!("{}", info_message.as_str());

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

        listeners_helper.deployment_in_progress(ProgressInfo::new(
            ProgressScope::Application {
                id: image.application_id.clone(),
            },
            ProgressLevel::Info,
            Some(info_message),
            self.context.execution_id(),
        ));

        match cmd::utilities::exec_with_envs(
            "docker",
            vec![
                "tag",
                dest.as_str(),
                format!("{}/{}", self.login.as_str(), dest.as_str()).as_str(),
            ],
            envs.clone(),
        ) {
            Err(_) => {
                return Err(self.engine_error(
                    EngineErrorCause::Internal,
                    format!("failed to tag image ({}) {:?}", image.name_with_tag(), image,),
                ));
            }
            _ => {}
        };

        match cmd::utilities::exec_with_envs("docker", vec!["push", dest.as_str()], envs) {
            Err(_) => {
                return Err(self.engine_error(
                    EngineErrorCause::Internal,
                    format!(
                        "failed to push image {:?} into DockerHub {}",
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

    fn push_error(&self, _image: &Image) -> Result<PushResult, EngineError> {
        unimplemented!()
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

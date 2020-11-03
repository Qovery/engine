use std::rc::Rc;

use crate::build_platform::Image;
use crate::cmd;
use crate::cmd::utilities::CmdError;
use crate::container_registry::{
    ContainerRegistry, ContainerRegistryError, Kind, PushError, PushResult,
};
use crate::models::{Context, Listener, Listeners, ProgressListener};

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

    fn is_valid(&self) -> Result<(), ContainerRegistryError> {
        // check the version of docker and print it as info
        let mut output_from_cmd = String::new();
        cmd::utilities::exec_with_output(
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

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }

    fn on_create(&self) -> Result<(), ContainerRegistryError> {
        Ok(())
    }

    fn on_create_error(&self) -> Result<(), ContainerRegistryError> {
        Ok(())
    }

    fn on_delete(&self) -> Result<(), ContainerRegistryError> {
        Ok(())
    }

    fn on_delete_error(&self) -> Result<(), ContainerRegistryError> {
        Ok(())
    }

    fn does_image_exists(&self, image: &Image) -> bool {
        let envs = match self.context.docker_tcp_socket() {
            Some(tcp_socket) => vec![("DOCKER_HOST", tcp_socket.as_str())],
            None => vec![],
        };

        // login into docker hub
        match cmd::utilities::exec_with_envs(
            "docker",
            vec![
                "login",
                "-u",
                self.login.as_str(),
                "-p",
                self.password.as_str(),
            ],
            envs.clone(),
        ) {
            Err(err) => match err {
                CmdError::Exec(exit_status) => {
                    error!("Cannot login into dockerhub");
                    return false;
                }
                CmdError::Io(err) => {
                    error!("IO error on dockerhub login: {}", err);
                    return false;
                }
                CmdError::Unexpected(err) => {
                    error!("Unexpected error on dockerhub login: {}", err);
                    return false;
                }
            },
            _ => {}
        };

        // check if image and tag exist
        // note: to retrieve if specific tags exist you can specify the tag at the end of the cUrl path
        let curl_path = format!(
            "https://index.docker.io/v1/repositories/{}/tags/",
            image.name
        );
        let mut exist_stdoud: bool = false;
        let mut exist_stderr: bool = true;

        cmd::utilities::exec_with_envs_and_output(
            "curl",
            vec!["--silent", "-f", "-lSL", &curl_path],
            envs.clone(),
            |r_out| match r_out {
                Ok(s) => exist_stdoud = true,
                Err(e) => error!("Error while getting stdout from curl {}", e),
            },
            |r_err| match r_err {
                Ok(s) => exist_stderr = true,
                Err(e) => error!("Error while getting stderr from curl {}", e),
            },
        );
        exist_stdoud
    }

    fn push(&self, image: &Image, force_push: bool) -> Result<PushResult, PushError> {
        let envs = match self.context.docker_tcp_socket() {
            Some(tcp_socket) => vec![("DOCKER_HOST", tcp_socket.as_str())],
            None => vec![],
        };

        match cmd::utilities::exec_with_envs(
            "docker",
            vec![
                "login",
                "-u",
                self.login.as_str(),
                "-p",
                self.password.as_str(),
            ],
            envs.clone(),
        ) {
            Err(err) => match err {
                CmdError::Exec(exit_status) => return Err(PushError::CredentialsError),
                CmdError::Io(err) => return Err(PushError::IoError(err)),
                CmdError::Unexpected(err) => return Err(PushError::Unknown(err)),
            },
            _ => {}
        };

        let dest = format!("{}/{}", self.login.as_str(), image.name_with_tag().as_str());
        match cmd::utilities::exec_with_envs(
            "docker",
            vec![
                "tag",
                dest.as_str(),
                format!("{}/{}", self.login.as_str(), dest.as_str()).as_str(),
            ],
            envs.clone(),
        ) {
            Err(err) => match err {
                CmdError::Exec(exit_status) => return Err(PushError::ImageTagFailed),
                CmdError::Io(err) => return Err(PushError::IoError(err)),
                CmdError::Unexpected(err) => return Err(PushError::Unknown(err)),
            },
            _ => {}
        };

        match cmd::utilities::exec_with_envs("docker", vec!["push", dest.as_str()], envs) {
            Err(err) => match err {
                CmdError::Exec(exit_status) => return Err(PushError::ImagePushFailed),
                CmdError::Io(err) => return Err(PushError::IoError(err)),
                CmdError::Unexpected(err) => return Err(PushError::Unknown(err)),
            },
            _ => {}
        };

        let mut image = image.clone();
        image.registry_url = Some(dest);

        Ok(PushResult { image })
    }

    fn push_error(&self, _image: &Image) -> Result<PushResult, PushError> {
        unimplemented!()
    }
}

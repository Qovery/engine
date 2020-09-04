use crate::build_platform::Image;
use crate::cmd;
use crate::cmd::CmdError;
use crate::container_registry::{
    ContainerRegistry, ContainerRegistryError, Kind, PushError, PushResult,
};
use crate::models::{Context, Listeners, ProgressListener};
use std::rc::Rc;

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
        // FIXME check docker binary availability
        Ok(())
    }

    fn add_listener(&mut self, listener: Rc<Box<dyn ProgressListener>>) {
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

    fn does_image_exists(&self, _image: &Image) -> bool {
        false // TODO check if image exists on the remote repository
    }

    fn push(&self, image: &Image, _force_push: bool) -> Result<PushResult, PushError> {
        match cmd::exec(
            "docker",
            vec![
                "login",
                "-u",
                self.login.as_str(),
                "-p",
                self.password.as_str(),
            ],
        ) {
            Err(err) => match err {
                CmdError::Exec(_exit_status) => return Err(PushError::CredentialsError),
                CmdError::Io(err) => panic!(err),
                CmdError::Unexpected(err) => panic!(err),
            },
            _ => {}
        };

        let dest = format!("{}/{}", self.login.as_str(), image.name_with_tag().as_str());
        match cmd::exec(
            "docker",
            vec![
                "tag",
                dest.as_str(),
                format!("{}/{}", self.login.as_str(), dest.as_str()).as_str(),
            ],
        ) {
            Err(err) => match err {
                CmdError::Exec(_exit_status) => return Err(PushError::ImageTagFailed),
                CmdError::Io(err) => panic!(err),
                CmdError::Unexpected(err) => panic!(err),
            },
            _ => {}
        };

        match cmd::exec("docker", vec!["push", dest.as_str()]) {
            Err(err) => match err {
                CmdError::Exec(_exit_status) => return Err(PushError::ImagePushFailed),
                CmdError::Io(err) => panic!(err),
                CmdError::Unexpected(err) => panic!(err),
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

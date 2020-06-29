use crate::build_platform::Image;
use crate::cmd;
use crate::cmd::CmdError;
use crate::container_registry::error::ContainerRegistryError;
use crate::container_registry::{ContainerRegistry, PushError, PushResult};

pub struct DockerHub<'a> {
    pub login: &'a str,
    pub password: &'a str,
}

impl<'a> DockerHub<'a> {
    pub fn new(login: &'a str, password: &'a str) -> Self {
        DockerHub { login, password }
    }
}

impl<'a> ContainerRegistry for DockerHub<'a> {
    fn is_valid(&self) -> Result<(), ContainerRegistryError> {
        Ok(())
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

    fn push(&self, image: Image) -> Result<PushResult, PushError> {
        match cmd::exec(
            "docker",
            vec!["login", "-u", self.login, "-p", self.password],
        ) {
            Err(err) => match err {
                CmdError::Io(err) => panic!(err),
                CmdError::Exec(exit_status) => return Err(PushError::CredentialsError),
            },
            _ => {}
        };

        let dest = format!("{}/{}", self.login, image.name_with_tag().as_str());
        match cmd::exec(
            "docker",
            vec![
                "tag",
                dest.as_str(),
                format!("{}/{}", self.login, dest.as_str()).as_str(),
            ],
        ) {
            Err(err) => match err {
                CmdError::Io(err) => panic!(err),
                CmdError::Exec(exit_status) => return Err(PushError::ImageTagFailed),
            },
            _ => {}
        };

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

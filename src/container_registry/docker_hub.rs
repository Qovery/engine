use crate::build_platform::Image;
use crate::cmd;
use crate::container_registry::{ContainerRegistry, PushError, PushResult};

pub struct DockerHub<'a> {
    pub login: &'a str,
    pub password: &'a str,
}

impl<'a> ContainerRegistry for DockerHub<'a> {
    fn is_valid(&self) -> bool {
        true
    }

    fn push(&self, image: &Image) -> Result<PushResult, PushError> {
        let status = match cmd::exec(
            "docker",
            vec!["login", "-u", self.login, "-p", self.password],
        ) {
            Ok(status) => status,
            Err(err) => panic!(err),
        };

        if !status.success() {
            return Err(PushError::CredentialsError);
        }

        let dest = format!("{}/{}", self.login, image.name_with_tag().as_str());
        let status = match cmd::exec(
            "docker",
            vec![
                "tag",
                dest.as_str(),
                format!("{}/{}", self.login, dest.as_str()).as_str(),
            ],
        ) {
            Ok(status) => status,
            Err(err) => panic!(err),
        };

        if !status.success() {
            return Err(PushError::ImageTagFailed);
        }

        let status = match cmd::exec("docker", vec!["push", dest.as_str()]) {
            Ok(status) => status,
            Err(err) => panic!(err),
        };

        if !status.success() {
            return Err(PushError::ImagePushFailed);
        }

        Ok(PushResult {})
    }
}

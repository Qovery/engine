extern crate digitalocean;

use std::rc::Rc;

use digitalocean::DigitalOcean;

use crate::build_platform::Image;
use crate::cmd;
use crate::cmd::utilities::CmdError;
use crate::container_registry::{
    ContainerRegistry, ContainerRegistryError, Kind, PushError, PushResult,
};
use crate::models::{Context, Listener, ProgressListener};

// TODO : use --output json
// see https://www.digitalocean.com/community/tutorials/how-to-use-doctl-the-official-digitalocean-command-line-client

pub struct DOCR {
    pub context: Context,
    pub registry_name: String,
    pub api_key: String,
}

impl DOCR {
    pub fn new(context: Context, registry_name: &str, api_key: &str) -> Self {
        DOCR {
            context,
            registry_name: registry_name.to_string(),
            api_key: api_key.to_string(),
        }
    }
    pub fn client(&self) -> DigitalOcean {
        DigitalOcean::new(self.api_key.as_str()).unwrap()
    }

    pub fn create_repository(&self, _image: &Image) -> Result<(), ContainerRegistryError> {
        match cmd::utilities::exec(
            "doctl",
            vec![
                "registry",
                "create",
                self.registry_name.as_str(),
                "-t",
                self.api_key.as_str(),
            ],
        ) {
            Err(err) => match err {
                CmdError::Exec(_exit_status) => return Err(ContainerRegistryError::Unknown),
                CmdError::Io(err) => return Err(ContainerRegistryError::Unknown),
                CmdError::Unexpected(err) => return Err(ContainerRegistryError::Unknown),
            },
            _ => {}
        };
        Ok(())
    }

    pub fn push_image(&self, dest: String, image: &Image) -> Result<PushResult, PushError> {
        match cmd::utilities::exec(
            "docker",
            vec!["tag", image.name_with_tag().as_str(), dest.as_str()],
        ) {
            Err(err) => match err {
                CmdError::Exec(_exit_status) => return Err(PushError::ImageTagFailed),
                CmdError::Io(err) => return Err(PushError::IoError(err)),
                CmdError::Unexpected(err) => return Err(PushError::Unknown(err)),
            },
            _ => {}
        };

        match cmd::utilities::exec("docker", vec!["push", dest.as_str()]) {
            Err(err) => match err {
                CmdError::Exec(_exit_status) => return Err(PushError::ImagePushFailed),
                CmdError::Io(err) => return Err(PushError::IoError(err)),
                CmdError::Unexpected(err) => return Err(PushError::Unknown(err)),
            },
            _ => {}
        };

        let mut image = image.clone();
        image.registry_url = Some(dest);

        Ok(PushResult { image })
    }

    fn get_or_create_repository(&self, _image: &Image) -> Result<(), ContainerRegistryError> {
        // TODO check if repository really exist
        self.create_repository(&_image)
    }

    fn delete_repository(&self, _image: &Image) -> Result<(), ContainerRegistryError> {
        match cmd::utilities::exec(
            "doctl",
            vec![
                "registry",
                "delete",
                self.registry_name.as_str(),
                "-f",
                "-t",
                self.api_key.as_str(),
            ],
        ) {
            Err(err) => match err {
                CmdError::Exec(exit_status) => return Err(ContainerRegistryError::Unknown),
                CmdError::Io(err) => return Err(ContainerRegistryError::Unknown),
                CmdError::Unexpected(err) => return Err(ContainerRegistryError::Unknown),
            },
            _ => {}
        };
        Ok(())
    }
}

impl ContainerRegistry for DOCR {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::DOCR
    }

    fn id(&self) -> &str {
        unimplemented!()
    }

    fn name(&self) -> &str {
        unimplemented!()
    }

    fn is_valid(&self) -> Result<(), ContainerRegistryError> {
        match cmd::doctl::doctl_do_registry_login(&self.api_key) {
            Ok(_o) => {}
            Err(e) => return Err(ContainerRegistryError::Credentials),
        };
        Ok(())
    }

    fn add_listener(&mut self, _listener: Listener) {
        unimplemented!()
    }

    fn on_create(&self) -> Result<(), ContainerRegistryError> {
        info!("Digital Ocean Container Registry.on_create() called");
        cmd::doctl::doctl_do_registry_create(&self.api_key);
        Ok(())
    }

    fn on_create_error(&self) -> Result<(), ContainerRegistryError> {
        unimplemented!()
    }

    fn on_delete(&self) -> Result<(), ContainerRegistryError> {
        unimplemented!()
    }

    fn on_delete_error(&self) -> Result<(), ContainerRegistryError> {
        unimplemented!()
    }

    fn does_image_exists(&self, _image: &Image) -> bool {
        unimplemented!()
    }

    // https://www.digitalocean.com/docs/images/container-registry/how-to/use-registry-docker-kubernetes/
    fn push(&self, image: &Image, _force_push: bool) -> Result<PushResult, PushError> {
        let image = image.clone();
        //TODO instead use get_or_create_repository
        self.create_repository(&image);
        match cmd::utilities::exec(
            "doctl",
            vec![
                "registry",
                "login",
                self.registry_name.as_str(),
                "-t",
                self.api_key.as_str(),
            ],
        ) {
            Err(err) => match err {
                CmdError::Exec(_exit_status) => return Err(PushError::CredentialsError),
                CmdError::Io(err) => return Err(PushError::IoError(err)),
                CmdError::Unexpected(err) => return Err(PushError::Unknown(err)),
            },
            _ => {}
        };
        //TODO check force or not
        let dest = format!("{}:{}", self.registry_name.as_str(), image.tag.as_str());
        self.push_image(dest, &image)
    }

    fn push_error(&self, _image: &Image) -> Result<PushResult, PushError> {
        unimplemented!()
    }
}

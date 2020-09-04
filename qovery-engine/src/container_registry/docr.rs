use crate::build_platform::Image;
use crate::cmd;
use crate::cmd::CmdError;
use crate::container_registry::{
    ContainerRegistry, ContainerRegistryError, Kind, PushError, PushResult,
};
extern crate digitalocean;
use crate::models::{Context, ProgressListener};
use digitalocean::DigitalOcean;
use std::rc::Rc;

pub struct DOCR {
    context: Context,
    registry_name: String,
    api_key: String,
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

    fn create_repository(&self, image: &Image) -> Result<(), ContainerRegistryError> {
        match cmd::exec(
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
                CmdError::Exec(exit_status) => return Err(ContainerRegistryError::Unknown),
                CmdError::Io(err) => panic!(err),
                CmdError::Unexpected(err) => panic!(err),
            },
            _ => {}
        };
        Ok(())
    }

    fn delete_repository(&self, image: &Image) -> Result<(), ContainerRegistryError> {
        match cmd::exec(
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
                CmdError::Io(err) => panic!(err),
                CmdError::Unexpected(err) => panic!(err),
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
        unimplemented!()
    }

    fn add_listener(&mut self, listener: Rc<Box<dyn ProgressListener>>) {
        unimplemented!()
    }

    fn on_create(&self) -> Result<(), ContainerRegistryError> {
        unimplemented!()
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

    fn does_image_exists(&self, image: &Image) -> bool {
        unimplemented!()
    }

    // https://www.digitalocean.com/docs/images/container-registry/how-to/use-registry-docker-kubernetes/
    fn push(&self, image: &Image, force_push: bool) -> Result<PushResult, PushError> {
        let mut image = image.clone();
        Ok(PushResult { image })
    }

    fn push_error(&self, image: &Image) -> Result<PushResult, PushError> {
        unimplemented!()
    }
}

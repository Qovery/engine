extern crate digitalocean;

use std::rc::Rc;

use digitalocean::DigitalOcean;

use crate::build_platform::Image;
use crate::cmd;
use crate::container_registry::{ContainerRegistry, EngineError, Kind, PushResult};
use crate::error::{EngineErrorCause, EngineErrorScope};
use crate::models::{Context, Listener, ProgressListener};

// TODO : use --output json
// see https://www.digitalocean.com/community/tutorials/how-to-use-doctl-the-official-digitalocean-command-line-client

pub struct DOCR {
    pub context: Context,
    pub registry_name: String,
    pub api_key: String,
    pub name: String,
    pub id: String,
}

impl DOCR {
    pub fn new(context: Context, id: &str, name: &str, registry_name: &str, api_key: &str) -> Self {
        DOCR {
            context,
            registry_name: registry_name.to_string(),
            api_key: api_key.to_string(),
            id: id.to_string(),
            name: name.to_string(),
        }
    }
    pub fn client(&self) -> DigitalOcean {
        DigitalOcean::new(self.api_key.as_str()).unwrap()
    }

    pub fn create_repository(&self, _image: &Image) -> Result<(), EngineError> {
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
            Err(_) => {
                return Err(self.engine_error(
                    EngineErrorCause::Internal,
                    format!("failed to create DOCR {}", self.registry_name.as_str()),
                ));
            }
            _ => {}
        };

        Ok(())
    }

    pub fn push_image(&self, dest: String, image: &Image) -> Result<PushResult, EngineError> {
        match cmd::utilities::exec(
            "docker",
            vec!["tag", image.name_with_tag().as_str(), dest.as_str()],
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

        match cmd::utilities::exec("docker", vec!["push", dest.as_str()]) {
            Err(_) => {
                return Err(self.engine_error(
                    EngineErrorCause::Internal,
                    format!(
                        "failed to push image {:?} into DOCR {}",
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

    fn get_or_create_repository(&self, _image: &Image) -> Result<(), EngineError> {
        // TODO check if repository really exist
        self.create_repository(&_image)
    }

    fn delete_repository(&self, _image: &Image) -> Result<(), EngineError> {
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
            Err(_) => {
                return Err(self.engine_error(
                    EngineErrorCause::Internal,
                    format!(
                        "failed to delete DOCR repository {} from {}",
                        self.registry_name.as_str(),
                        self.name_with_id(),
                    ),
                ));
            }
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
        self.id.as_str()
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn is_valid(&self) -> Result<(), EngineError> {
        match cmd::doctl::doctl_do_registry_login(&self.api_key) {
            Ok(_o) => {}
            Err(e) => return Err(
                self.engine_error(
                    EngineErrorCause::User("Your DOCR account seems to be no longer valid (bad Credentials). \
                    Please contact your Organization administrator to fix or change the Credentials."),
                    format!("failed to login to DOCR {}", self.name_with_id())))
        };
        Ok(())
    }

    fn add_listener(&mut self, _listener: Listener) {
        unimplemented!()
    }

    fn on_create(&self) -> Result<(), EngineError> {
        unimplemented!()
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

    fn does_image_exists(&self, _image: &Image) -> bool {
        unimplemented!()
    }

    // https://www.digitalocean.com/docs/images/container-registry/how-to/use-registry-docker-kubernetes/
    fn push(&self, image: &Image, _force_push: bool) -> Result<PushResult, EngineError> {
        let image = image.clone();
        // TODO 1/ instead use get_or_create_repository
        // TODO 2/ does an error is returned if the repository already exist or not?
        self.create_repository(&image)?;

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
            Err(_) => {
                return Err(
                    self.engine_error(
                        EngineErrorCause::User("Your DOCR account seems to be no longer valid (bad Credentials). \
                    Please contact your Organization administrator to fix or change the Credentials."),
                        format!("failed to login to DOCR {}", self.name_with_id()))
                );
            }
            _ => {}
        };

        //TODO check force or not
        let dest = format!("{}:{}", self.registry_name.as_str(), image.tag.as_str());
        self.push_image(dest, &image)
    }

    fn push_error(&self, _image: &Image) -> Result<PushResult, EngineError> {
        unimplemented!()
    }
}

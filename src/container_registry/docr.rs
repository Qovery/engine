extern crate digitalocean;

use std::rc::Rc;

use digitalocean::DigitalOcean;
use tracing::{event, span, Level};

use crate::build_platform::Image;
use crate::cmd;
use crate::container_registry::{ContainerRegistry, EngineError, Kind, PushResult};
use crate::error::{EngineErrorCause, EngineErrorScope};
use crate::models::{Context, Listener, ProgressListener};
use reqwest::blocking::{Client, Response};
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::{header, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Error;

// TODO : use --output json
// see https://www.digitalocean.com/community/tutorials/how-to-use-doctl-the-official-digitalocean-command-line-client

pub struct DOCR {
    pub context: Context,
    pub registry_name: String,
    pub api_key: String,
    pub name: String,
    pub id: String,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
struct DO_API_Create_repository {
    name: String,
    subscription_tier_slug: String,
}

const cr_api_path: &str = "https://api.digitalocean.com/v2/registry";

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
        let mut headers = get_header_with_bearer(&self.api_key);
        //TODO: receive subscription_tier_slug from core
        let repo = DO_API_Create_repository {
            name: self.registry_name.clone(),
            subscription_tier_slug: "basic".to_owned(),
        };
        let to_create_repo = serde_json::to_string(&repo);
        match to_create_repo {
            Ok(repo_res) => {
                let res = reqwest::blocking::Client::new()
                    .post(cr_api_path)
                    .headers(headers)
                    .body(repo_res)
                    .send();
                match res {
                    Ok(output) => match output.status() {
                        StatusCode::OK => Ok(()),
                        StatusCode::CREATED => Ok(()),
                        status => {
                            event!(Level::WARN, "status from DO registry API {}", status);
                            return Err(self.engine_error(
                                EngineErrorCause::Internal,
                                format!(
                                    "Bad status code : {} returned by the DO registry API for creating DO CR {}",
                                    status,
                                    &self.registry_name,
                                ),
                            ));
                        }
                    },
                    Err(e) => {
                        return Err(self.engine_error(
                            EngineErrorCause::Internal,
                            format!(
                                "failed to create repository {} : {:?}",
                                &self.registry_name, e,
                            ),
                        ))
                    }
                }
            }
            Err(e) => {
                return Err(self.engine_error(
                    EngineErrorCause::Internal,
                    format!(
                        "Unable to initialize DO Registry {} : {:?}",
                        &self.registry_name, e,
                    ),
                ))
            }
        }
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

    pub fn delete_repository(&self, _image: &Image) -> Result<(), EngineError> {
        let mut headers = get_header_with_bearer(&self.api_key);
        let res = reqwest::blocking::Client::new()
            .delete(cr_api_path)
            .headers(headers)
            .send();
        match res {
            Ok(out) => match out.status() {
                StatusCode::NO_CONTENT => Ok(()),
                status => {
                    event!(Level::WARN, "delete status from DO registry API {}", status);
                    return Err(self.engine_error(
                        EngineErrorCause::Internal,
                        format!(
                            "Bad status code : {} returned by the DO registry API for deleting DO CR {}",
                            status,
                            &self.registry_name,
                        ),
                    ));
                }
            },
            Err(e) => {
                return Err(self.engine_error(
                    EngineErrorCause::Internal,
                    format!(
                        "No response from the Digital Ocean API {} : {:?}",
                        &self.registry_name, e,
                    ),
                ))
            }
        }
    }
}

// generate the right header for digital ocean with token
fn get_header_with_bearer(token: &str) -> HeaderMap<HeaderValue> {
    let mut headers = header::HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse().unwrap());
    headers.insert(
        "Authorization",
        format!("Bearer {}", token).parse().unwrap(),
    );
    headers
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

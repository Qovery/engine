extern crate digitalocean;

use std::rc::Rc;

use digitalocean::DigitalOcean;
use reqwest::blocking::{Client, Response};
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::{header, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Error;

use crate::build_platform::Image;
use crate::cmd;
use crate::container_registry::{ContainerRegistry, EngineError, Kind, PushResult};
use crate::error::{EngineErrorCause, EngineErrorScope, SimpleError, SimpleErrorKind};
use crate::models::{
    Context, Listener, Listeners, ListenersHelper, ProgressInfo, ProgressLevel, ProgressScope,
};

// TODO : use --output json
// see https://www.digitalocean.com/community/tutorials/how-to-use-doctl-the-official-digitalocean-command-line-client

pub struct DOCR {
    pub context: Context,
    pub registry_name: String,
    pub api_key: String,
    pub id: String,
    pub listeners: Listeners,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
struct DO_API_Create_repository {
    name: String,
    subscription_tier_slug: String,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
struct DO_API_Subecribe_to_Kube_Cluster {
    cluster_uuids: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DO_API_get_container_registry {
    pub registry: Registry,
    pub subscription: Subscription,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Registry {
    pub name: String,
    #[serde(rename = "created_at")]
    pub created_at: String,
    #[serde(rename = "storage_usage_bytes")]
    pub storage_usage_bytes: i64,
    #[serde(rename = "storage_usage_updated_at")]
    pub storage_usage_updated_at: String,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    pub tier: Tier,
    #[serde(rename = "created_at")]
    pub created_at: String,
    #[serde(rename = "updated_at")]
    pub updated_at: String,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tier {
    pub name: String,
    pub slug: String,
    #[serde(rename = "included_repositories")]
    pub included_repositories: i64,
    #[serde(rename = "included_storage_bytes")]
    pub included_storage_bytes: i64,
    #[serde(rename = "allow_storage_overage")]
    pub allow_storage_overage: bool,
    #[serde(rename = "included_bandwidth_bytes")]
    pub included_bandwidth_bytes: i64,
    #[serde(rename = "monthly_price_in_cents")]
    pub monthly_price_in_cents: i64,
}

pub const CR_API_PATH: &str = "https://api.digitalocean.com/v2/registry";
pub const CR_CLUSTER_API_PATH: &str = "https://api.digitalocean.com/v2/kubernetes/registry";

impl DOCR {
    pub fn new(context: Context, id: &str, registry_name: &str, api_key: &str) -> Self {
        DOCR {
            context,
            registry_name: registry_name.to_string(),
            api_key: api_key.to_string(),
            id: id.to_string(),
            listeners: vec![],
        }
    }
    pub fn client(&self) -> DigitalOcean {
        DigitalOcean::new(self.api_key.as_str()).unwrap()
    }

    pub fn create_repository(&self, _image: &Image) -> Result<(), EngineError> {
        let mut headers = get_header_with_bearer(&self.api_key);
        // subscription_tier_slug: https://www.digitalocean.com/products/container-registry/
        // starter and basic tiers are too limited on repository creation
        let repo = DO_API_Create_repository {
            name: self.registry_name.clone(),
            subscription_tier_slug: "professional".to_owned(),
        };
        let to_create_repo = serde_json::to_string(&repo);
        match to_create_repo {
            Ok(repo_res) => {
                let res = reqwest::blocking::Client::new()
                    .post(CR_API_PATH)
                    .headers(headers)
                    .body(repo_res)
                    .send();
                match res {
                    Ok(output) => match output.status() {
                        StatusCode::OK => Ok(()),
                        StatusCode::CREATED => Ok(()),
                        status => {
                            warn!("status from DO registry API {}", status);
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
                        ));
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
                ));
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

        match cmd::utilities::exec_with_output(
            "docker",
            vec!["push", dest.as_str()],
            |r_out| match r_out {
                Ok(line) => info!("{}", line),
                Err(line) => error!("{}", line),
            },
            |r_out| match r_out {
                Ok(line) => info!("{}", line),
                Err(line) => error!("{}", line),
            },
        ) {
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
            .delete(CR_API_PATH)
            .headers(headers)
            .send();
        match res {
            Ok(out) => match out.status() {
                StatusCode::NO_CONTENT => Ok(()),
                status => {
                    warn!("delete status from DO registry API {}", status);
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
                ));
            }
        }
    }
}

pub fn path_to_retrieve_all_tags_from_registry(
    registry_name: String,
    repository_name: String,
) -> String {
    format!(
        "https://api.digitalocean.com/v2/registry/{}/repositories/{}/tags",
        registry_name, repository_name
    )
}

// generate the right header for digital ocean with token
pub fn get_header_with_bearer(token: &str) -> HeaderMap<HeaderValue> {
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
        self.registry_name.as_str()
    }

    fn is_valid(&self) -> Result<(), EngineError> {
        Ok(())
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }

    fn on_create(&self) -> Result<(), EngineError> {
        info!("DOCR.on_create() called");
        Ok(())
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

    fn does_image_exists(&self, image: &Image) -> bool {
        let mut headers = get_header_with_bearer(self.api_key.as_str());
        let res = reqwest::blocking::Client::new()
            .get(
                path_to_retrieve_all_tags_from_registry(
                    self.registry_name.clone(),
                    image.name.clone(),
                )
                .as_str(),
            )
            .headers(headers)
            .send();
        let body = match res {
            Ok(output) => match output.status() {
                StatusCode::OK => output.text(),
                _ => {
                    error!(
                        "While tyring to get all tags for image: {}, maybe this image not exist !",
                        &image.name
                    );
                    return false;
                }
            },
            Err(_) => {
                error!(
                    "While trying to communicate with DigitalOcean API to retrieve all tags for image {}",
                    &image.name
                );
                return false;
            }
        };
        match body {
            Ok(out) => {
                let body_de = serde_json::from_str::<DescribeTagsForImage>(&out);
                match body_de {
                    Ok(tags_list) => {
                        for tag_element in tags_list.tags {
                            if tag_element.tag.eq(&image.tag) {
                                return true;
                            }
                        }
                        false
                    }
                    Err(e) => {
                        error!(
                            "Unable to deserialize tags from  DigitalOcean API for image {}",
                            &image.tag
                        );
                        return false;
                    }
                }
            }
            _ => {
                error!(
                    "while retrieving tags for image {} Unable to get output from DigitalOcean API",
                    &image.name
                );
                return false;
            }
        }
    }

    // https://www.digitalocean.com/docs/images/container-registry/how-to/use-registry-docker-kubernetes/
    fn push(&self, image: &Image, _force_push: bool) -> Result<PushResult, EngineError> {
        let image = image.clone();
        match self.create_repository(&image) {
            Ok(_) => info!(
                "Digital Ocean Container registry {} is created",
                self.registry_name
            ),
            Err(_) => warn!("Unable to create Container registry {}", self.registry_name),
        };

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
        //TODO: check if image doesn't exist before pushing it!

        let dest = format!(
            "registry.digitalocean.com/{}/{}",
            self.registry_name.as_str(),
            image.name_with_tag()
        );
        let listeners_helper = ListenersHelper::new(&self.listeners);
        let info_message = format!(
            "image {:?} does not exist into DOCR {} repository - let's upload it",
            image,
            self.name()
        );

        listeners_helper.start_in_progress(ProgressInfo::new(
            ProgressScope::Application {
                id: image.application_id.clone(),
            },
            ProgressLevel::Info,
            Some(info_message),
            self.context.execution_id(),
        ));
        self.push_image(dest, &image)
    }

    fn push_error(&self, _image: &Image) -> Result<PushResult, EngineError> {
        unimplemented!()
    }
}

pub fn subscribe_kube_cluster_to_container_registry(
    api_key: &str,
    cluster_uuid: &str,
) -> Result<(), SimpleError> {
    let mut headers = get_header_with_bearer(api_key);
    let cluster_ids = DO_API_Subecribe_to_Kube_Cluster {
        cluster_uuids: vec![cluster_uuid.to_string()],
    };

    let res_cluster_to_link = serde_json::to_string(&cluster_ids);
    return match res_cluster_to_link {
        Ok(cluster_to_link) => {
            let res = reqwest::blocking::Client::new()
                .post(CR_CLUSTER_API_PATH)
                .headers(headers)
                .body(cluster_to_link)
                .send();
            match res {
                Ok(output) => match output.status() {
                    StatusCode::NO_CONTENT => Ok(()),
                    status => {
                        warn!("status from DO registry API {}", status);
                        Err(SimpleError::new(SimpleErrorKind::Other, Some("Incorrect Status received from Digital Ocean when tyring to subscribe repository to cluster")))
                    }
                },
                Err(e) => {
                    error!("{:?}", e);
                    Err(SimpleError::new(SimpleErrorKind::Other, Some("Unable to call Digital Ocean when tyring to subscribe repository to cluster")))
                }
            }
        }
        Err(e) => {
            error!("{:?}", e);
            Err(SimpleError::new(
                SimpleErrorKind::Other,
                Some("Unable to Serialize digital ocean cluster uuids"),
            ))
        }
    };
}

pub fn get_current_registry_name(api_key: &str) -> Result<String, SimpleError> {
    let headers = get_header_with_bearer(api_key);
    let res = reqwest::blocking::Client::new()
        .get(CR_API_PATH)
        .headers(headers)
        .send();
    match res {
        Ok(output) => match output.status() {
            StatusCode::OK => {
                let content = output.text().unwrap();
                let res_registry = serde_json::from_str::<DO_API_get_container_registry>(&content);

                match res_registry {
                    Ok(registry) => return Ok(registry.registry.name),
                    Err(e) => return Err(SimpleError::new(
                        SimpleErrorKind::Other,
                        Some(
                            "While trying to deserialize Registry describe json received from Digital Ocean API",
                        ),
                    )),
                }
            }
            status => {
                warn!("status from DO registry API {}", status);
                return Err(SimpleError::new(SimpleErrorKind::Other, Some("Incorrect Status received from Digital Ocean when tyring to subscribe repository to cluster")));
            }
        },
        Err(e) => {
            error!("{:?}", e);
            return Err(SimpleError::new(
                SimpleErrorKind::Other,
                Some(
                    "Unable to call Digital Ocean when tyring to fetch the container registry name",
                ),
            ));
        }
    }
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeTagsForImage {
    pub tags: Vec<Tag>,
    pub meta: Meta,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    #[serde(rename = "registry_name")]
    pub registry_name: String,
    pub repository: String,
    pub tag: String,
    #[serde(rename = "manifest_digest")]
    pub manifest_digest: String,
    #[serde(rename = "compressed_size_bytes")]
    pub compressed_size_bytes: i64,
    #[serde(rename = "size_bytes")]
    pub size_bytes: i64,
    #[serde(rename = "updated_at")]
    pub updated_at: String,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    pub total: i64,
}

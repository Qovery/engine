extern crate digitalocean;

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::borrow::Borrow;

use crate::build_platform::Image;
use crate::cmd::command::QoveryCommand;
use crate::container_registry::docker::{docker_pull_image, docker_tag_and_push_image};
use crate::container_registry::{ContainerRegistry, EngineError, Kind, PullResult, PushResult};
use crate::errors::CommandError;
use crate::events::{EngineEvent, EventDetails, EventMessage, ToTransmitter, Transmitter};
use crate::logger::{LogLevel, Logger};
use crate::models::{
    Context, Listen, Listener, Listeners, ListenersHelper, ProgressInfo, ProgressLevel, ProgressScope,
};
use crate::utilities;
use retry::delay::Fixed;
use retry::Error::Operation;
use retry::OperationResult;

const CR_API_PATH: &str = "https://api.digitalocean.com/v2/registry";
const CR_CLUSTER_API_PATH: &str = "https://api.digitalocean.com/v2/kubernetes/registry";

// TODO : use --output json
// see https://www.digitalocean.com/community/tutorials/how-to-use-doctl-the-official-digitalocean-command-line-client

pub struct DOCR {
    pub context: Context,
    pub name: String,
    pub api_key: String,
    pub id: String,
    pub listeners: Listeners,
    pub logger: Box<dyn Logger>,
}

impl DOCR {
    pub fn new(context: Context, id: &str, name: &str, api_key: &str, logger: Box<dyn Logger>) -> Self {
        DOCR {
            context,
            name: name.into(),
            api_key: api_key.into(),
            id: id.into(),
            listeners: vec![],
            logger,
        }
    }

    fn get_registry_name(&self, image: &Image) -> Result<String, EngineError> {
        let event_details = self.get_event_details();

        let registry_name = match image.registry_name.as_ref() {
            // DOCR does not support upper cases
            Some(registry_name) => registry_name.to_lowercase(),
            None => get_current_registry_name(self.api_key.as_str(), event_details, self.logger())?,
        };

        Ok(registry_name)
    }

    fn create_repository(&self, image: &Image) -> Result<(), EngineError> {
        let event_details = self.get_event_details();

        let registry_name = match image.registry_name.as_ref() {
            // DOCR does not support upper cases
            Some(registry_name) => registry_name.to_lowercase(),
            None => self.name.clone(),
        };

        let headers = utilities::get_header_with_bearer(&self.api_key);
        // subscription_tier_slug: https://www.digitalocean.com/products/container-registry/
        // starter and basic tiers are too limited on repository creation
        let repo = DoApiCreateRepository {
            name: registry_name.clone(),
            subscription_tier_slug: "professional".to_string(),
        };

        match serde_json::to_string(&repo) {
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
                            return Err(EngineError::new_container_registry_namespace_creation_error(
                                event_details.clone(),
                                self.name_with_id(),
                                registry_name.to_string(),
                                CommandError::new_from_safe_message(format!(
                                    "Bad status code: `{}` returned by the DO registry API for creating DOCR `{}`.",
                                    status,
                                    registry_name.as_str(),
                                )),
                            ));
                        }
                    },
                    Err(e) => {
                        return Err(EngineError::new_container_registry_namespace_creation_error(
                            event_details.clone(),
                            self.name_with_id(),
                            registry_name.to_string(),
                            CommandError::new(
                                e.to_string(),
                                Some(format!(
                                    "Failed to create DOCR repository `{}`.",
                                    registry_name.as_str(),
                                )),
                            ),
                        ));
                    }
                }
            }
            Err(e) => {
                return Err(EngineError::new_container_registry_namespace_creation_error(
                    event_details.clone(),
                    self.name_with_id(),
                    registry_name.to_string(),
                    CommandError::new(
                        e.to_string(),
                        Some(format!(
                            "Failed to create DOCR repository `{}`.",
                            registry_name.as_str(),
                        )),
                    ),
                ));
            }
        }
    }

    fn push_image(&self, registry_name: String, dest: String, image: &Image) -> Result<PushResult, EngineError> {
        let event_details = self.get_event_details();

        let dest_latest_tag = format!(
            "registry.digitalocean.com/{}/{}:latest",
            registry_name.as_str(),
            image.name
        );

        if let Err(e) = docker_tag_and_push_image(
            self.kind(),
            vec![],
            image,
            dest.clone(),
            dest_latest_tag.clone(),
            event_details.clone(),
            self.logger(),
        ) {
            return Err(EngineError::new_docker_push_image_error(
                event_details,
                image.name.to_string(),
                dest.to_string(),
                e,
            ));
        }

        let mut image = image.clone();
        image.registry_name = Some(registry_name.clone());
        // on DOCR registry secret is the same as registry name
        image.registry_secret = Some(registry_name);
        image.registry_url = Some(dest);

        let result = retry::retry(Fixed::from_millis(10000).take(12), || {
            match self.does_image_exists(&image) {
                true => OperationResult::Ok(&image),
                false => {
                    self.logger.log(
                        LogLevel::Warning,
                        EngineEvent::Warning(
                            self.get_event_details(),
                            EventMessage::new_from_safe(
                                "Image is not yet available on DOCR, retrying in a few seconds...".to_string(),
                            ),
                        ),
                    );
                    OperationResult::Retry(())
                }
            }
        });

        let image_not_reachable = Err(EngineError::new_container_registry_image_unreachable_after_push(
            event_details.clone(),
            image.name.to_string(),
        ));
        match result {
            Ok(_) => Ok(PushResult { image }),
            Err(Operation { .. }) => image_not_reachable,
            Err(retry::Error::Internal(_)) => image_not_reachable,
        }
    }

    pub fn get_image(&self, _image: &Image) -> Option<()> {
        todo!()
    }

    pub fn delete_image(&self, _image: &Image) -> Result<(), EngineError> {
        // TODO(benjaminch): To be implemented later on, but note it must not slow down CI workflow
        Ok(())
    }

    pub fn delete_repository(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details();

        let headers = utilities::get_header_with_bearer(&self.api_key);
        let res = reqwest::blocking::Client::new()
            .delete(CR_API_PATH)
            .headers(headers)
            .send();

        match res {
            Ok(out) => match out.status() {
                StatusCode::NO_CONTENT => Ok(()),
                status => {
                    return Err(EngineError::new_container_registry_delete_repository_error(
                        event_details.clone(),
                        "default".to_string(), // DO has only one repository
                        Some(CommandError::new_from_safe_message(format!(
                            "Bad status code: `{}` returned by the DO registry API for deleting DOCR.",
                            status,
                        ))),
                    ));
                }
            },
            Err(e) => {
                return Err(EngineError::new_container_registry_delete_repository_error(
                    event_details.clone(),
                    "default".to_string(), // DO has only one repository
                    Some(CommandError::new(
                        e.to_string(),
                        Some("No response from the Digital Ocean API.".to_string()),
                    )),
                ));
            }
        }
    }

    pub fn exec_docr_login(&self) -> Result<(), EngineError> {
        let event_details = self.get_event_details();

        let mut cmd = QoveryCommand::new(
            "doctl",
            &vec!["registry", "login", self.name.as_str(), "-t", self.api_key.as_str()],
            &vec![],
        );

        match cmd.exec() {
            Ok(_) => Ok(()),
            Err(_) => Err(EngineError::new_client_invalid_cloud_provider_credentials(
                event_details,
            )),
        }
    }

    fn pull_image(&self, registry_name: String, dest: String, image: &Image) -> Result<PullResult, EngineError> {
        let event_details = self.get_event_details();

        match docker_pull_image(self.kind(), vec![], dest.clone(), event_details.clone(), self.logger()) {
            Ok(_) => {
                let mut image = image.clone();
                image.registry_name = Some(registry_name.clone());
                // on DOCR registry secret is the same as registry name
                image.registry_secret = Some(registry_name);
                image.registry_url = Some(dest);
                Ok(PullResult::Some(image))
            }
            Err(e) => Err(EngineError::new_docker_pull_image_error(
                event_details,
                image.name.to_string(),
                dest.to_string(),
                e,
            )),
        }
    }
}

impl ToTransmitter for DOCR {
    fn to_transmitter(&self) -> Transmitter {
        Transmitter::ContainerRegistry(self.id().to_string(), self.name().to_string())
    }
}

impl ContainerRegistry for DOCR {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::Docr
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
        let event_details = self.get_event_details();

        let registry_name = match self.get_registry_name(image) {
            Ok(registry_name) => registry_name,
            Err(err) => {
                self.logger.log(LogLevel::Error, EngineEvent::Error(err, None));
                return false;
            }
        };

        let headers = utilities::get_header_with_bearer(self.api_key.as_str());
        let url = format!(
            "https://api.digitalocean.com/v2/registry/{}/repositories/{}/tags",
            registry_name,
            image.name.as_str()
        );

        let res = reqwest::blocking::Client::new()
            .get(url.as_str())
            .headers(headers)
            .send();

        let body = match res {
            Ok(output) => match output.status() {
                StatusCode::OK => output.text(),
                _ => {
                    self.logger.log(
                        LogLevel::Error,
                        EngineEvent::Error(
                            EngineError::new_container_registry_image_doesnt_exist(
                                event_details.clone(),
                                image.name.to_string(),
                                Some(CommandError::new_from_safe_message(format!(
                                    "While tyring to get all tags for image: `{}`, maybe this image not exist !",
                                    image.name.to_string()
                                ))),
                            ),
                            None,
                        ),
                    );

                    return false;
                }
            },
            Err(_) => {
                self.logger.log(
                    LogLevel::Error,
                    EngineEvent::Error(
                        EngineError::new_container_registry_image_doesnt_exist(
                            event_details.clone(),
                            image.name.to_string(),
                            Some(CommandError::new_from_safe_message(format!(
                                "While trying to communicate with DigitalOcean API to retrieve all tags for image `{}`.",
                                image.name.to_string()
                            ))),
                        ),
                        None,
                    ),
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
                    Err(_) => {
                        self.logger.log(
                            LogLevel::Error,
                            EngineEvent::Error(
                                EngineError::new_container_registry_image_doesnt_exist(
                                    event_details.clone(),
                                    image.name.to_string(),
                                    Some(CommandError::new(
                                        out.to_string(),
                                        Some(format!(
                                            "Unable to deserialize tags from DigitalOcean API for image {}",
                                            &image.tag.to_string(),
                                        )),
                                    )),
                                ),
                                None,
                            ),
                        );

                        false
                    }
                }
            }
            _ => {
                self.logger.log(
                    LogLevel::Error,
                    EngineEvent::Error(
                        EngineError::new_container_registry_image_doesnt_exist(
                            event_details.clone(),
                            image.name.to_string(),
                            Some(CommandError::new_from_safe_message(format!(
                                "While retrieving tags for image `{}` Unable to get output from DigitalOcean API.",
                                image.name.to_string()
                            ))),
                        ),
                        None,
                    ),
                );

                false
            }
        }
    }

    fn pull(&self, image: &Image) -> Result<PullResult, EngineError> {
        let event_details = self.get_event_details();
        let listeners_helper = ListenersHelper::new(&self.listeners);

        if !self.does_image_exists(image) {
            let info_message = format!("image {:?} does not exist in DOCR {} repository", image, self.name());

            self.logger.log(
                LogLevel::Info,
                EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe(info_message.to_string()),
                ),
            );

            listeners_helper.deployment_in_progress(ProgressInfo::new(
                ProgressScope::Application {
                    id: image.application_id.clone(),
                },
                ProgressLevel::Info,
                Some(info_message),
                self.context.execution_id(),
            ));

            return Ok(PullResult::None);
        }

        let info_message = format!("pull image {:?} from DOCR {} repository", image, self.name());

        self.logger.log(
            LogLevel::Info,
            EngineEvent::Info(
                event_details.clone(),
                EventMessage::new_from_safe(info_message.to_string()),
            ),
        );

        listeners_helper.deployment_in_progress(ProgressInfo::new(
            ProgressScope::Application {
                id: image.application_id.clone(),
            },
            ProgressLevel::Info,
            Some(info_message),
            self.context.execution_id(),
        ));

        let _ = self.exec_docr_login()?;

        let registry_name = self.get_registry_name(image)?;

        let dest = format!(
            "registry.digitalocean.com/{}/{}",
            registry_name.as_str(),
            image.name_with_tag()
        );

        // pull image
        self.pull_image(registry_name, dest, image)
    }

    // https://www.digitalocean.com/docs/images/container-registry/how-to/use-registry-docker-kubernetes/
    fn push(&self, image: &Image, force_push: bool) -> Result<PushResult, EngineError> {
        let event_details = self.get_event_details();
        let registry_name = self.get_registry_name(image)?;

        match self.create_repository(image) {
            Ok(_) => self.logger.log(
                LogLevel::Info,
                EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe(format!("DOCR {} has been created", registry_name.as_str())),
                ),
            ),
            Err(e) => self.logger.log(
                LogLevel::Error,
                EngineEvent::Error(
                    e.clone(),
                    Some(EventMessage::new_from_safe(format!(
                        "DOCR {} already exists",
                        registry_name.as_str()
                    ))),
                ),
            ),
        };

        let _ = self.exec_docr_login()?;

        let dest = format!(
            "registry.digitalocean.com/{}/{}",
            registry_name.as_str(),
            image.name_with_tag()
        );

        let listeners_helper = ListenersHelper::new(&self.listeners);

        if !force_push && self.does_image_exists(image) {
            // check if image does exist - if yes, do not upload it again
            let info_message = format!(
                "image {:?} found on DOCR {} repository, container build is not required",
                image,
                registry_name.as_str()
            );

            self.logger.log(
                LogLevel::Info,
                EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe(info_message.to_string()),
                ),
            );

            listeners_helper.deployment_in_progress(ProgressInfo::new(
                ProgressScope::Application {
                    id: image.application_id.clone(),
                },
                ProgressLevel::Info,
                Some(info_message),
                self.context.execution_id(),
            ));

            let mut image = image.clone();
            image.registry_name = Some(registry_name.clone());
            // on DOCR registry secret is the same as registry name
            image.registry_secret = Some(registry_name);
            image.registry_url = Some(dest);

            return Ok(PushResult { image });
        }

        let info_message = format!(
            "image {:?} does not exist on DOCR {} repository, starting image upload",
            image, registry_name
        );

        self.logger.log(
            LogLevel::Info,
            EngineEvent::Info(
                event_details.clone(),
                EventMessage::new_from_safe(info_message.to_string()),
            ),
        );

        listeners_helper.deployment_in_progress(ProgressInfo::new(
            ProgressScope::Application {
                id: image.application_id.clone(),
            },
            ProgressLevel::Info,
            Some(info_message),
            self.context.execution_id(),
        ));

        self.push_image(registry_name, dest, image)
    }

    fn push_error(&self, image: &Image) -> Result<PushResult, EngineError> {
        Ok(PushResult { image: image.clone() })
    }

    fn logger(&self) -> &dyn Logger {
        self.logger.borrow()
    }
}

impl Listen for DOCR {
    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }
}

pub fn subscribe_kube_cluster_to_container_registry(api_key: &str, cluster_uuid: &str) -> Result<(), CommandError> {
    let headers = utilities::get_header_with_bearer(api_key);
    let cluster_ids = DoApiSubscribeToKubeCluster {
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
                    status => Err(CommandError::new_from_safe_message(
                        format!("Incorrect Status `{}` received from Digital Ocean when tyring to subscribe repository to cluster", status)),
                    ),
                },
                Err(e) => Err(CommandError::new(
                    e.to_string(),
                    Some("Unable to call Digital Ocean when tyring to subscribe repository to cluster".to_string()),
                )),
            }
        }
        Err(e) => Err(CommandError::new(
            e.to_string(),
            Some("Unable to Serialize digital ocean cluster uuids".to_string()),
        )),
    };
}

pub fn get_current_registry_name(
    api_key: &str,
    event_details: EventDetails,
    logger: &dyn Logger,
) -> Result<String, EngineError> {
    let headers = utilities::get_header_with_bearer(api_key);
    let res = reqwest::blocking::Client::new()
        .get(CR_API_PATH)
        .headers(headers)
        .send();

    return match res {
        Ok(output) => match output.status() {
            StatusCode::OK => {
                let content = output.text().unwrap();
                let res_registry = serde_json::from_str::<DoApiGetContainerRegistry>(&content);

                match res_registry {
                    Ok(registry) => Ok(registry.registry.name),
                    Err(err) => Err(EngineError::new_container_registry_repository_doesnt_exist(
                        event_details.clone(),
                        "default".to_string(), // DO has only one repository
                        Some(CommandError::new(
                            err.to_string(),
                            Some(
                                "An error occurred while deserializing JSON coming from Digital Ocean API.".to_string(),
                            ),
                        )),
                    )),
                }
            }
            status => {
                Err(EngineError::new_container_registry_repository_doesnt_exist(
                    event_details.clone(),
                    "default".to_string(), // DO has only one repository
                    Some(CommandError::new(
                        format!("Status: {}", status),
                        Some(
                            "Incorrect Status received from Digital Ocean when tyring to get container registry."
                                .to_string(),
                        ),
                    )),
                ))
            }
        },
        Err(e) => {
            let err = EngineError::new_container_registry_repository_doesnt_exist(
                event_details.clone(),
                "default".to_string(), // DO has only one repository
                Some(CommandError::new(
                    e.to_string(),
                    Some("Unable to call Digital Ocean when tyring to fetch the container registry name.".to_string()),
                )),
            );

            logger.log(LogLevel::Error, EngineEvent::Error(err.clone(), None));

            Err(err)
        }
    };
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
struct DoApiCreateRepository {
    name: String,
    subscription_tier_slug: String,
}

#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
struct DoApiSubscribeToKubeCluster {
    cluster_uuids: Vec<String>,
}

#[derive(Default, Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DoApiGetContainerRegistry {
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

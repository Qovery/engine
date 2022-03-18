extern crate digitalocean;

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use std::borrow::Borrow;

use crate::build_platform::Image;
use crate::cmd::command::QoveryCommand;
use crate::cmd::docker::{to_engine_error, Docker};
use crate::container_registry::{ContainerRegistry, ContainerRegistryInfo, EngineError, Kind};
use crate::errors::CommandError;
use crate::events::{EngineEvent, EventDetails, ToTransmitter, Transmitter};
use crate::logger::{LogLevel, Logger};
use crate::models::{Context, Listen, Listener, Listeners};
use crate::utilities;
use url::Url;

const CR_API_PATH: &str = "https://api.digitalocean.com/v2/registry";
const CR_CLUSTER_API_PATH: &str = "https://api.digitalocean.com/v2/kubernetes/registry";
const CR_REGISTRY_DOMAIN: &str = "registry.digitalocean.com";

// TODO : use --output json
// see https://www.digitalocean.com/community/tutorials/how-to-use-doctl-the-official-digitalocean-command-line-client

pub struct DOCR {
    pub context: Context,
    pub name: String,
    pub api_key: String,
    pub id: String,
    pub registry_info: ContainerRegistryInfo,
    pub listeners: Listeners,
    pub logger: Box<dyn Logger>,
}

impl DOCR {
    pub fn new(
        context: Context,
        id: &str,
        name: &str,
        api_key: &str,
        logger: Box<dyn Logger>,
    ) -> Result<Self, EngineError> {
        let registry_name = name.to_string();
        let mut registry = Url::parse(&format!("https://{}", CR_REGISTRY_DOMAIN)).unwrap();
        let _ = registry.set_username(&api_key);
        let _ = registry.set_password(Some(&api_key));
        let registry_info = ContainerRegistryInfo {
            endpoint: registry,
            registry_name: name.to_string(),
            registry_docker_json_config: None,
            get_image_name: Box::new(move |img_name| format!("{}/{}", registry_name, img_name)),
        };

        let cr = DOCR {
            context,
            name: name.to_string(),
            api_key: api_key.into(),
            id: id.into(),
            registry_info,
            listeners: vec![],
            logger,
        };

        let event_details = cr.get_event_details();
        let docker =
            Docker::new(cr.context.docker_tcp_socket().clone()).map_err(|err| to_engine_error(&event_details, err))?;
        if docker.login(&cr.registry_info.endpoint).is_err() {
            return Err(EngineError::new_client_invalid_cloud_provider_credentials(
                event_details,
            ));
        }
        Ok(cr)
    }

    fn create_registry(&self, registry_name: &str) -> Result<(), EngineError> {
        let event_details = self.get_event_details();

        // DOCR does not support upper cases
        let registry_name = registry_name.to_lowercase();
        let headers = utilities::get_header_with_bearer(&self.api_key);
        // subscription_tier_slug: https://www.digitalocean.com/products/container-registry/
        // starter and basic tiers are too limited on repository creation
        let repo = DoApiCreateRepository {
            name: registry_name.to_string(),
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

    pub fn delete_registry(&self) -> Result<(), EngineError> {
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

    fn login(&self) -> Result<&ContainerRegistryInfo, EngineError> {
        Ok(&self.registry_info)
    }

    fn create_registry(&self) -> Result<(), EngineError> {
        // Digital Ocean only allow one registry per account...
        if let Err(_) = get_current_registry_name(self.api_key.as_str(), self.get_event_details(), self.logger()) {
            let _ = self.create_registry(self.name())?;
        }

        Ok(())
    }

    fn create_repository(&self, _repository_name: &str) -> Result<(), EngineError> {
        // Nothing to do, DO only allow one registry and create repository on the flight when image are pushed
        Ok(())
    }

    fn does_image_exists(&self, image: &Image) -> bool {
        let event_details = self.get_event_details();

        let headers = utilities::get_header_with_bearer(self.api_key.as_str());
        let url = format!(
            "https://api.digitalocean.com/v2/registry/{}/repositories/{}/tags",
            image.registry_name,
            image.name()
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
                                image.name().to_string(),
                                Some(CommandError::new_from_safe_message(format!(
                                    "While tyring to get all tags for image: `{}`, maybe this image not exist !",
                                    image.name().to_string()
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
                            image.name().to_string(),
                            Some(CommandError::new_from_safe_message(format!(
                                "While trying to communicate with DigitalOcean API to retrieve all tags for image `{}`.",
                                image.name().to_string()
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
                                    image.name().to_string(),
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
                            image.name().to_string(),
                            Some(CommandError::new_from_safe_message(format!(
                                "While retrieving tags for image `{}` Unable to get output from DigitalOcean API.",
                                image.name().to_string()
                            ))),
                        ),
                        None,
                    ),
                );

                false
            }
        }
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

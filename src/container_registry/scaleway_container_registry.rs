extern crate scaleway_api_rs;

use crate::cloud_provider::scaleway::application::ScwZone;
use std::borrow::Borrow;

use self::scaleway_api_rs::models::scaleway_registry_v1_namespace::Status;
use crate::build_platform::Image;
use crate::container_registry::docker::{
    docker_login, docker_manifest_inspect, docker_pull_image, docker_tag_and_push_image,
};
use crate::container_registry::{ContainerRegistry, Kind, PullResult, PushResult};
use crate::errors::{CommandError, EngineError};
use crate::events::{EngineEvent, EventMessage, ToTransmitter, Transmitter};
use crate::logger::{LogLevel, Logger};
use crate::models::{
    Context, Listen, Listener, Listeners, ListenersHelper, ProgressInfo, ProgressLevel, ProgressScope,
};
use crate::runtime::block_on;
use retry::delay::Fibonacci;
use retry::Error::Operation;
use retry::OperationResult;
use rusoto_core::param::ToParam;

pub struct ScalewayCR {
    context: Context,
    id: String,
    name: String,
    default_project_id: String,
    login: String,
    secret_token: String,
    zone: ScwZone,
    listeners: Listeners,
    logger: Box<dyn Logger>,
}

impl ScalewayCR {
    pub fn new(
        context: Context,
        id: &str,
        name: &str,
        secret_token: &str,
        default_project_id: &str,
        zone: ScwZone,
        logger: Box<dyn Logger>,
    ) -> ScalewayCR {
        ScalewayCR {
            context,
            id: id.to_string(),
            name: name.to_string(),
            default_project_id: default_project_id.to_string(),
            login: "nologin".to_string(),
            secret_token: secret_token.to_string(),
            zone,
            listeners: Vec::new(),
            logger,
        }
    }

    fn get_configuration(&self) -> scaleway_api_rs::apis::configuration::Configuration {
        scaleway_api_rs::apis::configuration::Configuration {
            api_key: Some(scaleway_api_rs::apis::configuration::ApiKey {
                key: self.secret_token.clone(),
                prefix: None,
            }),
            ..scaleway_api_rs::apis::configuration::Configuration::default()
        }
    }

    fn get_docker_envs(&self) -> Vec<(&str, &str)> {
        match self.context.docker_tcp_socket() {
            Some(tcp_socket) => vec![("DOCKER_HOST", tcp_socket.as_str())],
            None => vec![],
        }
    }

    pub fn get_registry_namespace(
        &self,
        image: &Image,
    ) -> Option<scaleway_api_rs::models::ScalewayRegistryV1Namespace> {
        // https://developers.scaleway.com/en/products/registry/api/#get-09e004
        let scaleway_registry_namespaces = match block_on(scaleway_api_rs::apis::namespaces_api::list_namespaces(
            &self.get_configuration(),
            self.zone.region().to_string().as_str(),
            None,
            None,
            None,
            None,
            Some(self.default_project_id.as_str()),
            image.registry_name.as_deref(),
        )) {
            Ok(res) => res.namespaces,
            Err(e) => {
                self.logger.log(
                    LogLevel::Warning,
                    EngineEvent::Warning(
                        self.get_event_details(),
                        EventMessage::new(
                            "Error while interacting with Scaleway API (list_namespaces).".to_string(),
                            Some(format!("error: {}, image: {}", e, &image.name)),
                        ),
                    ),
                );
                return None;
            }
        };

        // We consider every registry namespace names are unique
        if let Some(registries) = scaleway_registry_namespaces {
            if let Some(registry) = registries
                .into_iter()
                .filter(|r| r.status == Some(Status::Ready))
                .next()
            {
                return Some(registry);
            }
        }

        None
    }

    pub fn get_image(&self, image: &Image) -> Option<scaleway_api_rs::models::ScalewayRegistryV1Image> {
        // https://developers.scaleway.com/en/products/registry/api/#get-a6f1bc
        let scaleway_images = match block_on(scaleway_api_rs::apis::images_api::list_images(
            &self.get_configuration(),
            self.zone.region().to_string().as_str(),
            None,
            None,
            None,
            None,
            Some(image.name.as_str()),
            None,
            Some(self.default_project_id.as_str()),
        )) {
            Ok(res) => res.images,
            Err(e) => {
                self.logger.log(
                    LogLevel::Warning,
                    EngineEvent::Warning(
                        self.get_event_details(),
                        EventMessage::new(
                            "Error while interacting with Scaleway API (list_namespaces).".to_string(),
                            Some(format!("error: {}, image: {}", e, &image.name)),
                        ),
                    ),
                );
                return None;
            }
        };

        if let Some(images) = scaleway_images {
            // Scaleway doesn't allow to specify any tags while getting image
            // so we need to check if tags are the ones we are looking for
            for scaleway_image in images.into_iter() {
                if scaleway_image.tags.is_some() && scaleway_image.tags.as_ref().unwrap().contains(&image.tag) {
                    return Some(scaleway_image);
                }
            }
        }

        None
    }

    pub fn delete_image(&self, image: &Image) -> Result<scaleway_api_rs::models::ScalewayRegistryV1Image, EngineError> {
        let event_details = self.get_event_details();

        // https://developers.scaleway.com/en/products/registry/api/#delete-67dbf7
        let image_to_delete = self.get_image(image);
        if image_to_delete.is_none() {
            let err = EngineError::new_container_registry_image_doesnt_exist(
                event_details.clone(),
                image.name.to_string(),
                None,
            );

            self.logger.log(LogLevel::Error, EngineEvent::Error(err.clone(), None));

            return Err(err);
        }

        let image_to_delete = image_to_delete.unwrap();

        match block_on(scaleway_api_rs::apis::images_api::delete_image(
            &self.get_configuration(),
            self.zone.region().to_string().as_str(),
            image_to_delete.id.unwrap().as_str(),
        )) {
            Ok(res) => Ok(res),
            Err(e) => {
                let err = EngineError::new_container_registry_delete_image_error(
                    event_details.clone(),
                    image.name.to_string(),
                    Some(CommandError::new(e.to_string(), None)),
                );

                self.logger.log(LogLevel::Error, EngineEvent::Error(err.clone(), None));

                Err(err)
            }
        }
    }

    fn push_image(&self, dest: String, dest_latest_tag: String, image: &Image) -> Result<PushResult, EngineError> {
        // https://www.scaleway.com/en/docs/deploy-an-image-from-registry-to-kubernetes-kapsule/
        let event_details = self.get_event_details();

        if let Err(e) = docker_tag_and_push_image(
            self.kind(),
            self.get_docker_envs(),
            image,
            dest.to_string(),
            dest_latest_tag.to_string(),
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

        let result = retry::retry(Fibonacci::from_millis(10000).take(10), || {
            match self.does_image_exists(image) {
                true => OperationResult::Ok(&image),
                false => {
                    self.logger.log(
                        LogLevel::Warning,
                        EngineEvent::Warning(
                            self.get_event_details(),
                            EventMessage::new_from_safe(
                                "Image is not yet available on Scaleway Registry Namespace, retrying in a few seconds...".to_string(),
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
            Ok(_) => Ok(PushResult { image: image.clone() }),
            Err(Operation { .. }) => image_not_reachable,
            Err(retry::Error::Internal(_)) => image_not_reachable,
        }
    }

    fn pull_image(&self, dest: String, image: &Image) -> Result<PullResult, EngineError> {
        let event_details = self.get_event_details();

        if let Err(e) = docker_pull_image(
            self.kind(),
            self.get_docker_envs(),
            dest.to_string(),
            event_details.clone(),
            self.logger(),
        ) {
            return Err(EngineError::new_docker_pull_image_error(
                event_details,
                image.name.to_string(),
                dest.to_string(),
                e,
            ));
        }

        Ok(PullResult::Some(image.clone()))
    }

    pub fn create_registry_namespace(
        &self,
        image: &Image,
    ) -> Result<scaleway_api_rs::models::ScalewayRegistryV1Namespace, EngineError> {
        let event_details = self.get_event_details();

        // https://developers.scaleway.com/en/products/registry/api/#post-7a8fcc
        match block_on(scaleway_api_rs::apis::namespaces_api::create_namespace(
            &self.get_configuration(),
            self.zone.region().to_string().as_str(),
            scaleway_api_rs::models::inline_object_29::InlineObject29 {
                name: image.name.clone(),
                description: None,
                project_id: Some(self.default_project_id.clone()),
                is_public: Some(false),
                organization_id: None,
            },
        )) {
            Ok(res) => Ok(res),
            Err(e) => {
                let error = EngineError::new_container_registry_namespace_creation_error(
                    event_details.clone(),
                    image.name.clone(),
                    self.name_with_id(),
                    CommandError::new(e.to_string(), Some("Can't create SCW repository".to_string())),
                );

                self.logger
                    .log(LogLevel::Error, EngineEvent::Error(error.clone(), None));

                Err(error)
            }
        }
    }

    pub fn delete_registry_namespace(
        &self,
        image: &Image,
    ) -> Result<scaleway_api_rs::models::ScalewayRegistryV1Namespace, EngineError> {
        // https://developers.scaleway.com/en/products/registry/api/#delete-c1ac9b
        let event_details = self.get_event_details();
        let registry_to_delete = self.get_registry_namespace(image);
        let repository_name = match image.registry_name.as_ref() {
            None => "unknown",
            Some(name) => name,
        };
        if registry_to_delete.is_none() {
            let error = EngineError::new_container_registry_repository_doesnt_exist(
                event_details.clone(),
                repository_name.to_string(),
                None,
            );

            self.logger
                .log(LogLevel::Error, EngineEvent::Error(error.clone(), None));

            return Err(error);
        }

        let registry_to_delete = registry_to_delete.unwrap();

        match block_on(scaleway_api_rs::apis::namespaces_api::delete_namespace(
            &self.get_configuration(),
            self.zone.region().to_string().as_str(),
            registry_to_delete.id.unwrap().as_str(),
        )) {
            Ok(res) => Ok(res),
            Err(e) => {
                let error = EngineError::new_container_registry_delete_repository_error(
                    event_details.clone(),
                    repository_name.to_string(),
                    Some(CommandError::new(e.to_string(), None)),
                );

                self.logger
                    .log(LogLevel::Error, EngineEvent::Error(error.clone(), None));

                return Err(error);
            }
        }
    }

    pub fn get_or_create_registry_namespace(
        &self,
        image: &Image,
    ) -> Result<scaleway_api_rs::models::ScalewayRegistryV1Namespace, EngineError> {
        // check if the repository already exists
        let event_details = self.get_event_details();
        let registry_namespace = self.get_registry_namespace(&image);
        if let Some(namespace) = registry_namespace {
            self.logger.log(
                LogLevel::Info,
                EngineEvent::Info(
                    event_details.clone(),
                    EventMessage::new_from_safe(format!("SCW repository {} already exists", image.name.as_str())),
                ),
            );
            return Ok(namespace);
        }

        self.create_registry_namespace(image)
    }

    fn get_docker_json_config_raw(&self) -> String {
        base64::encode(
            format!(
                r#"{{"auths":{{"rg.{}.scw.cloud":{{"auth":"{}"}}}}}}"#,
                self.zone.region().as_str(),
                base64::encode(format!("nologin:{}", self.secret_token).as_bytes())
            )
            .as_bytes(),
        )
    }

    fn exec_docker_login(&self, registry_url: &String) -> Result<(), EngineError> {
        let event_details = self.get_event_details();
        if docker_login(
            Kind::ScalewayCr,
            self.get_docker_envs(),
            self.login.clone(),
            self.secret_token.clone(),
            registry_url.clone(),
            event_details.clone(),
            self.logger(),
        )
        .is_err()
        {
            return Err(EngineError::new_client_invalid_cloud_provider_credentials(
                event_details,
            ));
        };

        Ok(())
    }
}

impl ToTransmitter for ScalewayCR {
    fn to_transmitter(&self) -> Transmitter {
        Transmitter::ContainerRegistry(self.id().to_string(), self.name().to_string())
    }
}

impl ContainerRegistry for ScalewayCR {
    fn context(&self) -> &Context {
        &self.context
    }

    fn kind(&self) -> Kind {
        Kind::ScalewayCr
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
        let registry_url = image
            .registry_url
            .as_ref()
            .unwrap_or(&"undefined".to_string())
            .to_param();

        if let Err(_) = docker_login(
            Kind::ScalewayCr,
            self.get_docker_envs(),
            self.login.clone(),
            self.secret_token.clone(),
            registry_url.clone(),
            event_details.clone(),
            self.logger(),
        ) {
            return false;
        }

        docker_manifest_inspect(
            Kind::ScalewayCr,
            self.get_docker_envs(),
            image.name.clone(),
            image.tag.clone(),
            registry_url,
            event_details.clone(),
            self.logger(),
        )
        .is_ok()
    }

    fn pull(&self, image: &Image) -> Result<PullResult, EngineError> {
        let event_details = self.get_event_details();
        let listeners_helper = ListenersHelper::new(&self.listeners);

        let mut image = image.clone();
        let registry_url: String;

        match self.get_or_create_registry_namespace(&image) {
            Ok(registry) => {
                self.logger.log(
                    LogLevel::Info,
                    EngineEvent::Info(
                        event_details.clone(),
                        EventMessage::new_from_safe(format!(
                            "Scaleway registry namespace for {} has been created",
                            image.name.as_str()
                        )),
                    ),
                );
                image.registry_name = Some(image.name.clone()); // Note: Repository namespace should have the same name as the image name
                image.registry_url = registry.endpoint.clone();
                image.registry_secret = Some(self.secret_token.clone());
                image.registry_docker_json_config = Some(self.get_docker_json_config_raw());
                registry_url = registry.endpoint.unwrap_or_else(|| "undefined".to_string());
            }
            Err(e) => {
                self.logger.log(LogLevel::Error, EngineEvent::Error(e.clone(), None));
                return Err(e);
            }
        }

        if !self.does_image_exists(&image) {
            let info_message = format!("Image {:?} does not exist in SCR {} repository", image, self.name());

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

        let info_message = format!("pull image {:?} from SCR {} repository", image, self.name());

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

        let _ = self.exec_docker_login(&registry_url)?;

        let dest = format!("{}/{}", registry_url, image.name_with_tag());

        // pull image
        self.pull_image(dest, &image)
    }

    fn push(&self, image: &Image, force_push: bool) -> Result<PushResult, EngineError> {
        let event_details = self.get_event_details();
        let mut image = image.clone();
        let registry_url: String;
        let registry_name: String;

        match self.get_or_create_registry_namespace(&image) {
            Ok(registry) => {
                image.registry_name = Some(image.name.clone()); // Note: Repository namespace should have the same name as the image name
                image.registry_url = registry.endpoint.clone();
                image.registry_secret = Some(self.secret_token.clone());
                image.registry_docker_json_config = Some(self.get_docker_json_config_raw());
                registry_url = registry.endpoint.unwrap_or_else(|| "undefined".to_string());
                registry_name = registry.name.unwrap();
            }
            Err(e) => {
                self.logger.log(LogLevel::Error, EngineEvent::Error(e.clone(), None));
                return Err(e);
            }
        }

        let _ = self.exec_docker_login(&registry_url)?;

        let dest = format!("{}/{}", registry_url, image.name_with_tag());

        let listeners_helper = ListenersHelper::new(&self.listeners);

        if !force_push && self.does_image_exists(&image) {
            // check if image does exist - if yes, do not upload it again
            let info_message = format!(
                "image {} found on Scaleway {} repository, container build is not required",
                image, registry_name,
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

            return Ok(PushResult { image: image.clone() });
        }

        let info_message = format!(
            "image {} does not exist on Scaleway {} repository, starting image upload",
            image,
            self.name()
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

        let dest_latest_tag = format!("{}/{}:latest", registry_url, image.name);
        self.push_image(dest, dest_latest_tag, &image)
    }

    fn push_error(&self, image: &Image) -> Result<PushResult, EngineError> {
        Ok(PushResult { image: image.clone() })
    }

    fn logger(&self) -> &dyn Logger {
        self.logger.borrow()
    }
}

impl Listen for ScalewayCR {
    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }
}

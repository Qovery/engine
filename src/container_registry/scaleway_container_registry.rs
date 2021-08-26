extern crate scaleway_api_rs;

use crate::cloud_provider::scaleway::application::Zone;

use crate::build_platform::Image;
use crate::cmd;
use crate::container_registry::utilities::docker_tag_and_push_image;
use crate::container_registry::{ContainerRegistry, Kind, PushResult};
use crate::error::{EngineError, EngineErrorCause};
use crate::models::{
    Context, Listen, Listener, Listeners, ListenersHelper, ProgressInfo, ProgressLevel, ProgressScope,
};
use crate::runtime::block_on;
use retry::delay::Fixed;
use retry::Error::Operation;
use retry::OperationResult;

pub struct ScalewayCR {
    context: Context,
    id: String,
    name: String,
    default_project_id: String,
    secret_token: String,
    zone: Zone,
    listeners: Listeners,
}

impl ScalewayCR {
    pub fn new(
        context: Context,
        id: &str,
        name: &str,
        secret_token: &str,
        default_project_id: &str,
        region: Zone,
    ) -> ScalewayCR {
        ScalewayCR {
            context,
            id: id.to_string(),
            name: name.to_string(),
            default_project_id: default_project_id.to_string(),
            secret_token: secret_token.to_string(),
            zone: region,
            listeners: Vec::new(),
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
            self.zone.to_string().as_str(),
            None,
            None,
            None,
            None,
            Some(self.default_project_id.as_str()),
            image.registry_name.as_deref(),
        )) {
            Ok(res) => res.namespaces,
            Err(e) => {
                error!(
                    "Error while interacting with Scaleway API (list_namespaces), error: {}, image: {}",
                    e, &image.name
                );
                return None;
            }
        };

        // We consider every registry namespace names are unique
        if let Some(registry) = scaleway_registry_namespaces {
            if !registry.is_empty() {
                return Some(registry.into_iter().next().unwrap());
            }
        }

        None
    }

    pub fn get_image(&self, image: &Image) -> Option<scaleway_api_rs::models::ScalewayRegistryV1Image> {
        // https://developers.scaleway.com/en/products/registry/api/#get-a6f1bc
        let scaleway_images = match block_on(scaleway_api_rs::apis::images_api::list_images1(
            &self.get_configuration(),
            self.zone.to_string().as_str(),
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
                error!(
                    "Error while interacting with Scaleway API (list_images), error: {}, image: {}",
                    e, &image.name
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
        // https://developers.scaleway.com/en/products/registry/api/#delete-67dbf7
        let image_to_delete = self.get_image(image);
        if image_to_delete.is_none() {
            let message = format!("While tyring to delete image {}, image doesn't exist", &image.name,);
            error!("{}", message);

            return Err(self.engine_error(EngineErrorCause::Internal, message));
        }

        let image_to_delete = image_to_delete.unwrap();

        match block_on(scaleway_api_rs::apis::images_api::delete_image1(
            &self.get_configuration(),
            self.zone.to_string().as_str(),
            image_to_delete.id.unwrap().as_str(),
        )) {
            Ok(res) => Ok(res),
            Err(e) => {
                let message = format!(
                    "Error while interacting with Scaleway API (delete_image), error: {}, image: {}",
                    e, &image.name
                );

                error!("{}", message);
                Err(self.engine_error(EngineErrorCause::Internal, message))
            }
        }
    }

    fn push_image(&self, image_url: String, image: &Image) -> Result<PushResult, EngineError> {
        // https://www.scaleway.com/en/docs/deploy-an-image-from-registry-to-kubernetes-kapsule/
        match docker_tag_and_push_image(
            self.kind(),
            self.get_docker_envs(),
            image.name.clone(),
            image.tag.clone(),
            image_url.clone(),
        ) {
            Ok(_) => {}
            Err(e) => {
                return Err(self.engine_error(
                    EngineErrorCause::Internal,
                    e.message
                        .unwrap_or_else(|| "unknown error occurring during docker push".to_string()),
                ))
            }
        };

        let result = retry::retry(Fixed::from_millis(10000).take(12), || {
            match self.does_image_exists(&image) {
                true => OperationResult::Ok(&image),
                false => {
                    warn!("image is not yet available on Scaleway Registry Namespace, retrying in a few seconds...");
                    OperationResult::Retry(())
                }
            }
        });

        let image_not_reachable = Err(self.engine_error(
            EngineErrorCause::Internal,
            "image has been pushed on Scaleway Registry Namespace but is not yet available after 2min. Please try to redeploy in a few minutes".to_string(),
        ));

        match result {
            Ok(_) => Ok(PushResult { image: image.clone() }),
            Err(Operation { .. }) => image_not_reachable,
            Err(retry::Error::Internal(_)) => image_not_reachable,
        }
    }

    pub fn create_registry_namespace(
        &self,
        image: &Image,
    ) -> Result<scaleway_api_rs::models::ScalewayRegistryV1Namespace, EngineError> {
        // https://developers.scaleway.com/en/products/registry/api/#post-7a8fcc
        match block_on(scaleway_api_rs::apis::namespaces_api::create_namespace(
            &self.get_configuration(),
            self.zone.to_string().as_str(),
            scaleway_api_rs::models::inline_object_23::InlineObject23 {
                name: image.name.clone(),
                description: None,
                project_id: Some(self.default_project_id.clone()),
                is_public: Some(false),
                organization_id: None,
            },
        )) {
            Ok(res) => Ok(res),
            Err(e) => {
                let message = format!(
                    "Error while interacting with Scaleway API (create_namespace), error: {}, image: {}",
                    e, &image.name
                );

                error!("{}", message);
                Err(self.engine_error(EngineErrorCause::Internal, message))
            }
        }
    }

    pub fn delete_registry_namespace(
        &self,
        image: &Image,
    ) -> Result<scaleway_api_rs::models::ScalewayRegistryV1Namespace, EngineError> {
        // https://developers.scaleway.com/en/products/registry/api/#delete-c1ac9b
        let registry_to_delete = self.get_registry_namespace(image);
        if registry_to_delete.is_none() {
            let message = format!(
                "While tyring to delete registry namespace for image {}, registry namespace doesn't exist",
                &image.name,
            );
            error!("{}", message);

            return Err(self.engine_error(EngineErrorCause::Internal, message));
        }

        let registry_to_delete = registry_to_delete.unwrap();

        match block_on(scaleway_api_rs::apis::namespaces_api::delete_namespace(
            &self.get_configuration(),
            self.zone.to_string().as_str(),
            registry_to_delete.id.unwrap().as_str(),
        )) {
            Ok(res) => Ok(res),
            Err(e) => {
                let message = format!(
                    "Error while interacting with Scaleway API (delete_namespace), error: {}, image: {}",
                    e, &image.name
                );

                error!("{}", message);
                Err(self.engine_error(EngineErrorCause::Internal, message))
            }
        }
    }

    pub fn get_or_create_registry_namespace(
        &self,
        image: &Image,
    ) -> Result<scaleway_api_rs::models::ScalewayRegistryV1Namespace, EngineError> {
        // check if the repository already exists
        let registry_namespace = self.get_registry_namespace(&image);
        if let Some(namespace) = registry_namespace {
            info!("Scaleway registry namespace {} already exists", image.name.as_str());
            return Ok(namespace);
        }

        self.create_registry_namespace(&image)
    }

    fn get_docker_json_config_raw(&self) -> String {
        base64::encode(
            format!(
                r#"{{"auths":{{"rg.{}.scw.cloud":{{"auth":"{}"}}}}}}"#,
                self.zone.as_str(),
                base64::encode(format!("nologin:{}", self.secret_token).as_bytes())
            )
            .as_bytes(),
        )
    }
}

impl ContainerRegistry for ScalewayCR {
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
        self.get_image(image).is_some()
    }

    fn push(&self, image: &Image, force_push: bool) -> Result<PushResult, EngineError> {
        let mut image = image.clone();
        let registry_url: String;
        let registry_name: String;

        match self.get_or_create_registry_namespace(&image) {
            Ok(registry) => {
                info!(
                    "Scaleway registry namespace for {} has been created",
                    image.name.as_str()
                );
                image.registry_name = Some(image.name.clone()); // Note: Repository namespace should have the same name as the image name
                image.registry_url = registry.endpoint.clone();
                image.registry_secret = Some(self.secret_token.clone());
                image.registry_docker_json_config = Some(self.get_docker_json_config_raw());
                registry_url = registry.endpoint.unwrap_or_else(|| "undefined".to_string());
                registry_name = registry.name.unwrap();
            }
            Err(e) => {
                error!(
                    "Scaleway registry namespace for {} cannot be created, error: {:?}",
                    image.name.as_str(),
                    e
                );
                return Err(e);
            }
        }

        let envs = self.get_docker_envs();

        if cmd::utilities::exec(
            "docker",
            vec![
                "login",
                registry_url.as_str(),
                "-u",
                "nologin",
                "-p",
                self.secret_token.as_str(),
            ],
            &envs,
        )
        .is_err()
        {
            return Err(self.engine_error(
                EngineErrorCause::User(
                    "Your Scaleway account seems to be no longer valid (bad Credentials). \
                Please contact your Organization administrator to fix or change the Credentials.",
                ),
                format!("failed to login to Scaleway {}", self.name_with_id()),
            ));
        };

        let image_url = format!("{}/{}", registry_url, image.name_with_tag());

        let listeners_helper = ListenersHelper::new(&self.listeners);

        if !force_push && self.does_image_exists(&image) {
            // check if image does exist - if yes, do not upload it again
            let info_message = format!(
                "image {:?} found on Scaleway {} repository, container build is not required",
                image, registry_name,
            );

            info!("{}", info_message.as_str());

            listeners_helper.deployment_in_progress(ProgressInfo::new(
                ProgressScope::Application {
                    id: image.application_id.clone(),
                },
                ProgressLevel::Info,
                Some(info_message),
                self.context.execution_id(),
            ));

            let image = image.clone();

            return self.push_image(image_url, &image);
        }

        let info_message = format!(
            "image {:?} does not exist on Scaleway {} repository, starting image upload",
            image,
            self.name()
        );

        info!("{}", info_message.as_str());

        listeners_helper.deployment_in_progress(ProgressInfo::new(
            ProgressScope::Application {
                id: image.application_id.clone(),
            },
            ProgressLevel::Info,
            Some(info_message),
            self.context.execution_id(),
        ));

        self.push_image(image_url, &image)
    }

    fn push_error(&self, image: &Image) -> Result<PushResult, EngineError> {
        Ok(PushResult { image: image.clone() })
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

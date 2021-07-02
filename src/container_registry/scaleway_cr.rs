extern crate scaleway_api_rs;

use crate::cloud_provider::scaleway::application::Region;

use crate::build_platform::Image;
use crate::container_registry::utilities::docker_tag_and_push_image;
use crate::container_registry::{ContainerRegistry, Kind, PushResult};
use crate::error::{cast_simple_error_to_engine_error, EngineError, EngineErrorCause, SimpleError, SimpleErrorKind};
use crate::models::{
    Context, Listen, Listener, Listeners, ListenersHelper, ProgressInfo, ProgressLevel, ProgressScope,
};
use crate::runtime::block_on;
use crate::{cmd, utilities};
use async_trait::async_trait;
use reqwest::StatusCode;
use retry::delay::Fixed;
use retry::Error::Operation;
use retry::OperationResult;
use serde_json::json;

pub struct ScalewayCR {
    context: Context,
    id: String,
    name: String,
    secret_token: String,
    default_project_id: String,
    region: Region,
    listeners: Listeners,
}

impl ScalewayCR {
    pub fn new(
        context: Context,
        id: String,
        name: String,
        secret_token: String,
        default_project_id: String,
        region: Region,
    ) -> ScalewayCR {
        ScalewayCR {
            context,
            id,
            name,
            secret_token,
            default_project_id,
            region,
            listeners: Vec::new(),
        }
    }

    async fn get_registry_namespace(&self, image: &Image) -> Option<scaleway_api_rs::models::ScalewayRegistryV1Namespace> {
        // https://developers.scaleway.com/en/products/registry/api/#get-09e004
        let configuration = scaleway_api_rs::apis::configuration::Configuration {
            bearer_access_token: Some(self.secret_token.clone()),
            ..scaleway_api_rs::apis::configuration::Configuration::default()
        };

        let scaleway_registry_namespaces = match scaleway_api_rs::apis::namespaces_api::list_namespaces(
            &configuration,
            self.region.to_string().as_str(),
            None,
            None,
            None,
            None,
            Some(self.default_project_id.as_str()),
            image.registry_name.as_deref(),
        )
        .await
        {
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
            if registry.len() > 0 {
                return Some(registry.into_iter().nth(0).unwrap());
            }
        }

        None
    }

    async fn get_image(&self, image: &Image) -> Option<scaleway_api_rs::models::ScalewayRegistryV1Image> {
        // https://developers.scaleway.com/en/products/registry/api/#get-a6f1bc
        let configuration = scaleway_api_rs::apis::configuration::Configuration {
            bearer_access_token: Some(self.secret_token.clone()),
            ..scaleway_api_rs::apis::configuration::Configuration::default()
        };

        let scaleway_images = match scaleway_api_rs::apis::images_api::list_images1(
            &configuration,
            self.region.to_string().as_str(),
            None,
            None,
            None,
            None,
            Some(image.name.as_str()),
            None,
            Some(image.registry_name.as_ref().unwrap_or(&"".to_string()).as_str()),
        )
        .await
        {
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

    pub async fn create_registry_namespace(&self, image: &Image) -> Result<scaleway_api_rs::models::ScalewayRegistryV1Namespace, EngineError> {
        // https://developers.scaleway.com/en/products/registry/api/#post-7a8fcc
        let configuration = scaleway_api_rs::apis::configuration::Configuration {
            bearer_access_token: Some(self.secret_token.clone()),
            ..scaleway_api_rs::apis::configuration::Configuration::default()
        };

        match scaleway_api_rs::apis::namespaces_api::create_namespace(
            &configuration,
            self.region.to_string().as_str(),
            scaleway_api_rs::models::inline_object_23::InlineObject23{
                name: image.registry_name.to_owned().unwrap(),
                description: None,
                project_id: Some(self.default_project_id.clone()),
                is_public: Some(false),
                organization_id: None,
            },
        ).await {
            Ok(res) => Ok(res),
            Err(e) => {
                let message = format!(
                    "Error while interacting with Scaleway API (create_namespace), error: {}, image: {}",
                    e, &image.name);

                error!("{}", message);
                Err(self.engine_error(EngineErrorCause::Internal, message))
            }
        }
    }

    pub async fn delete_registry_namespace(&self, image: &Image) -> Result<scaleway_api_rs::models::ScalewayRegistryV1Namespace, EngineError> {
        // https://developers.scaleway.com/en/products/registry/api/#delete-c1ac9b
        let registry_to_delete = self.get_registry_namespace(image).await;
        if registry_to_delete.is_none() {
            let message = format!(
                "While tyring to delete registry namespace for image {}, registry namespace doesn't exist",
                &image.name,
            );
            error!("{}", message);

            return Err(self.engine_error(EngineErrorCause::Internal, message));
        }

        let headers = utilities::get_header_with_bearer(self.secret_token.as_str());
        let url = format!(
            "https://api.scaleway.com/registry/v1/regions/{}/namespaces/{:?}",
            self.region.to_string().as_str(),
            registry_to_delete.unwrap().id,
        );

        let res = reqwest::blocking::Client::new()
            .delete(url.as_str())
            .headers(headers)
            .send();

        let body = match res {
            Ok(output) => match output.status() {
                StatusCode::OK => output.text(),
                _ => {
                    let message = format!(
                        "While tyring to delete registry namespace for image {}, Scaleway API error (status {}): {:?}",
                        &image.name,
                        &output.status(),
                        &output.text(),
                    );
                    error!("{}", message);

                    return Err(self.engine_error(EngineErrorCause::Internal, message));
                }
            },
            Err(_) => {
                let message = format!(
                    "While trying to communicate with Scaleway API to delete registry namespace image {}",
                    &image.name
                );
                error!("{}", message);

                return Err(self.engine_error(EngineErrorCause::Internal, message));
            }
        };

        match serde_json::from_str::<scaleway_api_rs::models::ScalewayRegistryV1Namespace>(&body.unwrap()) {
            Ok(res) => Ok(res),
            Err(e) => {
                let message = format!(
                    "While trying to deserialize Scaleway registry namespace response, image {}",
                    &image.name
                );
                error!("{}", message);

                return Err(self.engine_error(EngineErrorCause::Internal, message));
            }
        }
    }

    async fn get_or_create_registry_namespace(&self, image: &Image) -> Result<scaleway_api_rs::models::ScalewayRegistryV1Namespace, EngineError> {
        // check if the repository already exists
        let registry_namespace = self.get_registry_namespace(&image).await;
        if registry_namespace.is_some() {
            info!("Scaleway registry namespace {} already exists", image.name.as_str());
            return Ok(registry_namespace.unwrap());
        }

        self.create_registry_namespace(&image).await
    }
}


#[async_trait]
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

    async fn does_image_exists(&self, image: &Image) -> bool {
        self.get_image(image).await.is_some()
    }

    fn push(&self, image: &Image, force_push: bool) -> Result<PushResult, EngineError> {
        return Err(self.engine_error(
            EngineErrorCause::User("TODO(benjaminch): To be implemented"),
            format!("TODO(benjaminch): To be implemented {}", self.name_with_id()),
        ));
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

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

    fn get_registry_namespace(&self, image: &Image) -> Option<ScalewayRegistryNamespace> {
        // https://developers.scaleway.com/en/products/registry/api/#get-09e004
        let headers = utilities::get_header_with_bearer(self.secret_token.as_str());
        let url = format!(
            "https://api.scaleway.com/registry/v1/regions/{}/namespaces",
            self.region.to_string().as_str(),
        );

        let res = reqwest::blocking::Client::new()
            .get(url.as_str())
            .headers(headers)
            .query(&[
                ("project_id", self.default_project_id.as_str()),
                ("name", image.registry_name.as_ref().unwrap_or(&"".to_string())),
            ])
            .send();

        let body = match res {
            Ok(output) => match output.status() {
                StatusCode::OK => output.text(),
                _ => {
                    error!(
                        "While tyring to get registry namespace: {}, maybe this registry namespace doesn't exist !",
                        image.registry_name.as_ref().unwrap_or(&"".to_string()),
                    );
                    return None;
                }
            },
            Err(_) => {
                error!(
                    "While trying to communicate with Scaleway API to retrieve registry namespace {}",
                    image.registry_name.as_ref().unwrap_or(&"".to_string()),
                );
                return None;
            }
        };

        let scaleway_registry_namespaces = match serde_json::from_str::<ScalewayRegistryNamespaces>(&body.unwrap()) {
            Ok(res) => res.namespaces,
            Err(e) => {
                error!(
                    "While trying to deserialize Scaleway registry namespaces response, image {}",
                    &image.name
                );
                return None;
            }
        };

        // We consider every registry namespace names are unique
        match scaleway_registry_namespaces.len() {
            0 => None,
            _ => Some(scaleway_registry_namespaces.first().unwrap().clone()),
        }
    }

    fn get_image(&self, image: &Image) -> Option<ScalewayImage> {
        // https://developers.scaleway.com/en/products/registry/api/#get-a6f1bc
        let headers = utilities::get_header_with_bearer(self.secret_token.as_str());
        let url = format!(
            "https://api.scaleway.com/registry/v1/regions/{}/images",
            self.region.to_string().as_str(),
        );

        let res = reqwest::blocking::Client::new()
            .get(url.as_str())
            .headers(headers)
            .query(&[
                ("project_id", self.default_project_id.as_str()),
                ("name", image.registry_name.as_ref().unwrap_or(&"".to_string())),
            ])
            .send();

        let body = match res {
            Ok(output) => match output.status() {
                StatusCode::OK => output.text(),
                _ => {
                    error!(
                        "While tyring to get image: {}, maybe this image not exist !",
                        &image.name
                    );
                    return None;
                }
            },
            Err(_) => {
                error!(
                    "While trying to communicate with Scaleway API to retrieve image {}",
                    &image.name
                );
                return None;
            }
        };

        let scaleway_images = match serde_json::from_str::<ScalewayImages>(&body.unwrap()) {
            Ok(res) => res.images,
            Err(e) => {
                error!(
                    "While trying to deserialize Scaleway images response, image {}",
                    &image.name
                );
                return None;
            }
        };

        // Scaleway doesn't allow to specify any tags while getting image
        // so we need to check if tags are the ones we are looking for
        for scaleway_image in scaleway_images.into_iter() {
            if scaleway_image.tags.contains(&image.tag) {
                return Some(scaleway_image);
            }
        }

        // No images found with given tags
        None
    }

    pub fn create_registry_namespace(&self, image: &Image) -> Result<ScalewayRegistryNamespace, EngineError> {
        // https://developers.scaleway.com/en/products/registry/api/#post-7a8fcc
        let headers = utilities::get_header_with_bearer(self.secret_token.as_str());
        let url = format!(
            "https://api.scaleway.com/registry/v1/regions/{}/namespaces",
            self.region.to_string().as_str(),
        );

        let empty_field_value = "";
        let registry_namespace_name = image.registry_name.as_deref().unwrap_or(empty_field_value);

        let res = reqwest::blocking::Client::new()
            .post(url.as_str())
            .headers(headers)
            .json(&[
                ("name", registry_namespace_name),
                ("description", registry_namespace_name),
                ("project_id", self.default_project_id.as_str()),
                ("is_public", "false"),
            ])
            .send();

        let body = match res {
            Ok(output) => match output.status() {
                StatusCode::OK => output.text(),
                _ => {
                    let message = format!(
                        "While tyring to create registry namespace for image {}, Scaleway API error (status {}): {:?}",
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
                    "While trying to communicate with Scaleway API to create registry namespace image {}",
                    &image.name
                );
                error!("{}", message);

                return Err(self.engine_error(EngineErrorCause::Internal, message));
            }
        };

        match serde_json::from_str::<ScalewayRegistryNamespace>(&body.unwrap()) {
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

    pub fn delete_registry_namespace(&self, image: &Image) -> Result<ScalewayRegistryNamespace, EngineError> {
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

        let headers = utilities::get_header_with_bearer(self.secret_token.as_str());
        let url = format!(
            "https://api.scaleway.com/registry/v1/regions/{}/namespaces/{}",
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

        match serde_json::from_str::<ScalewayRegistryNamespace>(&body.unwrap()) {
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

    fn get_or_create_registry_namespace(&self, image: &Image) -> Result<ScalewayRegistryNamespace, EngineError> {
        // check if the repository already exists
        let registry_namespace = self.get_registry_namespace(&image);
        if registry_namespace.is_some() {
            info!("Scaleway registry namespace {} already exists", image.name.as_str());
            return Ok(registry_namespace.unwrap());
        }

        self.create_registry_namespace(&image)
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

#[derive(Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
pub struct ScalewayRegistryNamespaces {
    #[serde(rename = "namespaces")]
    namespaces: Vec<ScalewayRegistryNamespace>,
}

#[derive(Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
pub struct ScalewayRegistryNamespace {
    #[serde(rename = "id")]
    id: String,
    #[serde(rename = "name")]
    name: String,
    #[serde(rename = "description")]
    description: String,
    #[serde(rename = "organization_id")]
    organization_id: String,
    #[serde(rename = "project_id")]
    project_id: String,
    #[serde(rename = "status")]
    status: ScalewayResourceStatus,
    #[serde(rename = "status_message")]
    status_message: String,
    #[serde(rename = "endpoint")]
    endpoint: String,
    #[serde(rename = "is_public")]
    is_public: bool,
    #[serde(rename = "size")]
    size: u32,
    #[serde(rename = "created_at")]
    created_at: String,
    #[serde(rename = "updated_at")]
    updated_at: String,
    #[serde(rename = "image_count")]
    image_count: u32,
    #[serde(rename = "region")]
    region: Region,
}

#[derive(Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
pub struct ScalewayImages {
    #[serde(rename = "images")]
    images: Vec<ScalewayImage>,
}

#[derive(Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
pub struct ScalewayImage {
    #[serde(rename = "id")]
    id: String,
    #[serde(rename = "name")]
    name: String,
    #[serde(rename = "namespace_id")]
    namespace_id: String,
    #[serde(rename = "status")]
    status: ScalewayResourceStatus,
    #[serde(rename = "status_message")]
    status_message: String,
    #[serde(rename = "visibility")]
    visibility: ScalewayImageVisibility,
    #[serde(rename = "size")]
    size: u32,
    #[serde(rename = "created_at")]
    created_at: String,
    #[serde(rename = "updated_at")]
    updated_at: String,
    #[serde(rename = "tags")]
    tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
pub enum ScalewayResourceStatus {
    #[serde(rename = "unknown")]
    Unknown,
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "deleting")]
    Deleting,
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "locked")]
    Locked,
}

#[derive(Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
pub enum ScalewayImageVisibility {
    #[serde(rename = "visibility_unknown")]
    Unknown,
    #[serde(rename = "inherit")]
    Inherit,
    #[serde(rename = "public")]
    Public,
    #[serde(rename = "private")]
    Private,
}

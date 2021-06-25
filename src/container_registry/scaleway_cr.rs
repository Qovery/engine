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
        let headers = utilities::get_header_with_bearer(self.secret_token.as_str());
        let url = format!(
            "https://api.scaleway.com/registry/v1/regions/{}/images/{}",
            self.region.to_string().as_str(),
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
                    error!(
                        "While tyring to get all tags for image: {}, maybe this image not exist !",
                        &image.name
                    );
                    return false;
                }
            },
            Err(_) => {
                error!(
                    "While trying to communicate with Scaleway API to retrieve all tags for image {}",
                    &image.name
                );
                return false;
            }
        };

        let scaleway_image = match serde_json::from_str::<ScalewayImage>(&body.unwrap()) {
            Ok(image) => image,
            Err(e) => {
                error!(
                    "While trying to deserialize Scaleway image response, image {}",
                    &image.name
                );
                return false;
            }
        };

        // Scaleway doesn't allow to specify any repository (namespace) not tags while getting image
        // so we need to check if namespace and tags are the ones we are looking for
        (image.registry_name.is_some() && scaleway_image.namespace_id == *image.registry_name.as_ref().unwrap()
            || image.registry_name.is_none())
            && scaleway_image.tags.contains(&image.tag)
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

// https://developers.scaleway.com/en/products/registry/api/#get-1380f4
#[derive(Debug, Clone, PartialEq, serde_derive::Serialize, serde_derive::Deserialize)]
struct ScalewayImage {
    #[serde(rename = "id")]
    id: String,
    #[serde(rename = "name")]
    name: String,
    #[serde(rename = "namespace_id")]
    namespace_id: String,
    #[serde(rename = "status")]
    status: ScalewayImageStatus,
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
enum ScalewayImageStatus {
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
enum ScalewayImageVisibility {
    #[serde(rename = "visibility_unknown")]
    Unknown,
    #[serde(rename = "inherit")]
    Inherit,
    #[serde(rename = "public")]
    Public,
    #[serde(rename = "private")]
    Private,
}

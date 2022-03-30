extern crate scaleway_api_rs;

use self::scaleway_api_rs::models::scaleway_registry_v1_namespace::Status;
use crate::build_platform::Image;
use crate::cmd::docker;
use crate::container_registry::errors::ContainerRegistryError;
use crate::container_registry::{ContainerRegistry, ContainerRegistryInfo, Kind};
use crate::io_models::{Context, Listen, Listener, Listeners};
use crate::models::scaleway::ScwZone;
use crate::runtime::block_on;
use url::Url;

pub struct ScalewayCR {
    context: Context,
    id: String,
    name: String,
    default_project_id: String,
    secret_token: String,
    zone: ScwZone,
    registry_info: ContainerRegistryInfo,
    listeners: Listeners,
}

impl ScalewayCR {
    pub fn new(
        context: Context,
        id: &str,
        name: &str,
        secret_token: &str,
        default_project_id: &str,
        zone: ScwZone,
        listener: Listener,
    ) -> Result<ScalewayCR, ContainerRegistryError> {
        // Be sure we are logged on the registry
        let login = "nologin".to_string();
        let secret_token = secret_token.to_string();

        let mut registry = Url::parse(&format!("https://rg.{}.scw.cloud", zone.region())).unwrap();
        let _ = registry.set_username(&login);
        let _ = registry.set_password(Some(&secret_token));

        if context.docker.login(&registry).is_err() {
            return Err(ContainerRegistryError::InvalidCredentials);
        }

        let registry_info = ContainerRegistryInfo {
            endpoint: registry,
            registry_name: name.to_string(),
            registry_docker_json_config: Some(Self::get_docker_json_config_raw(
                &login,
                &secret_token,
                zone.region().as_str(),
            )),
            get_image_name: Box::new(move |img_name| format!("{}/{}", img_name, img_name)),
            get_repository_name: Box::new(|img_name| img_name.to_string()),
        };

        let cr = ScalewayCR {
            context,
            id: id.to_string(),
            name: name.to_string(),
            default_project_id: default_project_id.to_string(),
            secret_token,
            zone,
            registry_info,
            listeners: vec![listener],
        };

        Ok(cr)
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

    pub fn get_registry_namespace(
        &self,
        namespace_name: &str,
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
            Some(namespace_name),
        )) {
            Ok(res) => res.namespaces,
            Err(_e) => {
                return None;
            }
        };

        // We consider every registry namespace names are unique
        if let Some(registries) = scaleway_registry_namespaces {
            if let Some(registry) = registries.into_iter().find(|r| r.status == Some(Status::Ready)) {
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
            Some(image.name().as_str()),
            None,
            Some(self.default_project_id.as_str()),
        )) {
            Ok(res) => res.images,
            Err(_e) => {
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

    pub fn delete_image(
        &self,
        image: &Image,
    ) -> Result<scaleway_api_rs::models::ScalewayRegistryV1Image, ContainerRegistryError> {
        // https://developers.scaleway.com/en/products/registry/api/#delete-67dbf7
        let image_to_delete = self.get_image(image);
        if image_to_delete.is_none() {
            return Err(ContainerRegistryError::ImageDoesntExistInRegistry {
                registry_name: self.name.to_string(),
                repository_name: image.registry_name.to_string(),
                image_name: image.name.to_string(),
            });
        }

        let image_to_delete = image_to_delete.unwrap();

        match block_on(scaleway_api_rs::apis::images_api::delete_image(
            &self.get_configuration(),
            self.zone.region().to_string().as_str(),
            image_to_delete.id.unwrap().as_str(),
        )) {
            Ok(res) => Ok(res),
            Err(e) => Err(ContainerRegistryError::CannotDeleteImage {
                registry_name: self.name.to_string(),
                repository_name: image.registry_name.to_string(),
                image_name: image.name.to_string(),
                raw_error_message: e.to_string(),
            }),
        }
    }

    pub fn create_registry_namespace(
        &self,
        namespace_name: &str,
    ) -> Result<scaleway_api_rs::models::ScalewayRegistryV1Namespace, ContainerRegistryError> {
        // https://developers.scaleway.com/en/products/registry/api/#post-7a8fcc
        match block_on(scaleway_api_rs::apis::namespaces_api::create_namespace(
            &self.get_configuration(),
            self.zone.region().to_string().as_str(),
            scaleway_api_rs::models::inline_object_29::InlineObject29 {
                name: namespace_name.to_string(),
                description: None,
                project_id: Some(self.default_project_id.clone()),
                is_public: Some(false),
                organization_id: None,
            },
        )) {
            Ok(res) => Ok(res),
            Err(e) => Err(ContainerRegistryError::CannotCreateRepository {
                registry_name: self.name.to_string(),
                repository_name: namespace_name.to_string(),
                raw_error_message: e.to_string(),
            }),
        }
    }

    pub fn delete_registry_namespace(
        &self,
        namespace_name: &str,
    ) -> Result<scaleway_api_rs::models::ScalewayRegistryV1Namespace, ContainerRegistryError> {
        // https://developers.scaleway.com/en/products/registry/api/#delete-c1ac9b
        let registry_to_delete = self.get_registry_namespace(namespace_name);
        if registry_to_delete.is_none() {
            return Err(ContainerRegistryError::RepositoryDoesntExistInRegistry {
                registry_name: self.name.to_string(),
                repository_name: namespace_name.to_string(),
            });
        }

        let registry_to_delete = registry_to_delete.unwrap();

        match block_on(scaleway_api_rs::apis::namespaces_api::delete_namespace(
            &self.get_configuration(),
            self.zone.region().to_string().as_str(),
            registry_to_delete.id.unwrap().as_str(),
        )) {
            Ok(res) => Ok(res),
            Err(e) => Err(ContainerRegistryError::CannotDeleteRepository {
                registry_name: self.name.to_string(),
                repository_name: namespace_name.to_string(),
                raw_error_message: e.to_string(),
            }),
        }
    }

    pub fn get_or_create_registry_namespace(
        &self,
        namespace_name: &str,
    ) -> Result<scaleway_api_rs::models::ScalewayRegistryV1Namespace, ContainerRegistryError> {
        info!("Get/Create repository for {}", namespace_name);

        // check if the repository already exists
        let registry_namespace = self.get_registry_namespace(namespace_name);
        if let Some(namespace) = registry_namespace {
            return Ok(namespace);
        }

        self.create_registry_namespace(namespace_name)
    }

    fn get_docker_json_config_raw(login: &str, secret_token: &str, region: &str) -> String {
        base64::encode(
            format!(
                r#"{{"auths":{{"rg.{}.scw.cloud":{{"auth":"{}"}}}}}}"#,
                region,
                base64::encode(format!("{}:{}", login, secret_token).as_bytes())
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
        Kind::ScalewayCr
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn registry_info(&self) -> &ContainerRegistryInfo {
        &self.registry_info
    }

    fn create_registry(&self) -> Result<(), ContainerRegistryError> {
        // Nothing to do, scaleway managed container registry per repository (aka `namespace` by the scw naming convention)
        Ok(())
    }

    fn create_repository(&self, name: &str) -> Result<(), ContainerRegistryError> {
        let _ = self.get_or_create_registry_namespace(name)?;
        Ok(())
    }

    fn does_image_exists(&self, image: &Image) -> bool {
        let image = docker::ContainerImage {
            registry: self.registry_info.endpoint.clone(),
            name: image.name(),
            tags: vec![image.tag.clone()],
        };
        match self.context.docker.does_image_exist_remotely(&image) {
            Ok(true) => true,
            Ok(false) => false,
            Err(_) => false,
        }
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

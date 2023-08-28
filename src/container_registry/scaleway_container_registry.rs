extern crate scaleway_api_rs;

use self::scaleway_api_rs::models::scaleway_registry_v1_namespace::Status;
use crate::build_platform::Image;
use crate::cmd::docker;
use crate::container_registry::errors::{ContainerRegistryError, RepositoryNamingRule};
use crate::container_registry::{ContainerRegistry, ContainerRegistryInfo, Kind, RepositoryInfo};
use crate::io_models::context::Context;
use crate::models::scaleway::ScwZone;
use crate::runtime::block_on;
use std::collections::HashSet;
use url::Url;
use uuid::Uuid;

pub struct ScalewayCR {
    context: Context,
    id: String,
    long_id: Uuid,
    name: String,
    default_project_id: String,
    secret_token: String,
    zone: ScwZone,
    registry_info: ContainerRegistryInfo,
}

impl ScalewayCR {
    pub fn new(
        context: Context,
        id: &str,
        long_id: Uuid,
        name: &str,
        secret_token: &str,
        default_project_id: &str,
        zone: ScwZone,
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
            get_image_name: Box::new(move |img_name| format!("{img_name}/{img_name}")),
            get_repository_name: Box::new(|img_name| img_name.to_string()),
        };

        let cr = ScalewayCR {
            context,
            id: id.to_string(),
            long_id,
            name: name.to_string(),
            default_project_id: default_project_id.to_string(),
            secret_token,
            zone,
            registry_info,
        };

        Ok(cr)
    }

    fn check_repository_naming_rules(name: String) -> Option<HashSet<RepositoryNamingRule>> {
        let mut broken_rules = HashSet::new();

        if name.len() < 4 {
            broken_rules.insert(RepositoryNamingRule::MinLengthNotReached { min_length: 4 });
        }
        if name.len() > 54 {
            broken_rules.insert(RepositoryNamingRule::MaxLengthReached { max_length: 54 });
        }
        if !name.chars().all(|x| x.is_alphanumeric() || x == '-' || x == '.') {
            broken_rules.insert(RepositoryNamingRule::AlphaNumericCharsDashesPeriodsOnly);
        }

        match broken_rules.is_empty() {
            true => None,
            false => Some(broken_rules),
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
        if let Some(broken_rules) = ScalewayCR::check_repository_naming_rules(namespace_name.to_string()) {
            return Err(ContainerRegistryError::RepositoryNameNotValid {
                registry_name: self.name.to_string(),
                repository_name: namespace_name.to_string(),
                broken_rules,
            });
        }

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
    ) -> Result<(scaleway_api_rs::models::ScalewayRegistryV1Namespace, RepositoryInfo), ContainerRegistryError> {
        info!("Get/Create repository for {}", namespace_name);

        // check if the repository already exists
        let registry_namespace = self.get_registry_namespace(namespace_name);
        if let Some(namespace) = registry_namespace {
            return Ok((namespace, RepositoryInfo { created: false }));
        }

        let namespace = self.create_registry_namespace(namespace_name)?;
        Ok((namespace, RepositoryInfo { created: true }))
    }

    fn get_docker_json_config_raw(login: &str, secret_token: &str, region: &str) -> String {
        base64::encode(
            format!(
                r#"{{"auths":{{"rg.{}.scw.cloud":{{"auth":"{}"}}}}}}"#,
                region,
                base64::encode(format!("{login}:{secret_token}").as_bytes())
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

    fn long_id(&self) -> &Uuid {
        &self.long_id
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

    fn create_repository(
        &self,
        name: &str,
        _image_retention_time_in_seconds: u32,
    ) -> Result<RepositoryInfo, ContainerRegistryError> {
        let (_, repository_info) = self.get_or_create_registry_namespace(name)?;
        Ok(repository_info)
    }

    fn delete_repository(&self, repository_name: &str) -> Result<(), ContainerRegistryError> {
        if self.get_registry_namespace(repository_name).is_none() {
            return Ok(());
        }

        let _ = self.delete_registry_namespace(repository_name)?;
        Ok(())
    }

    fn delete_image(&self, image: &Image) -> Result<(), ContainerRegistryError> {
        match self.delete_image(image) {
            Ok(_) => Ok(()),
            Err(ContainerRegistryError::ImageDoesntExistInRegistry { .. }) => Ok(()),
            Err(err) => Err(err),
        }
    }

    fn does_image_exists(&self, image: &Image) -> bool {
        let image =
            docker::ContainerImage::new(self.registry_info.endpoint.clone(), image.name(), vec![image.tag.clone()]);
        match self.context.docker.does_image_exist_remotely(&image) {
            Ok(true) => true,
            Ok(false) => false,
            Err(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::container_registry::errors::RepositoryNamingRule;
    use crate::container_registry::scaleway_container_registry::ScalewayCR;
    use std::collections::HashSet;
    use std::iter::FromIterator;

    #[test]
    fn test_scaleway_container_registry_repository_naming_rules() {
        // setup:
        struct TestCase {
            input: String,
            expected: Option<HashSet<RepositoryNamingRule>>,
        }

        let test_cases = vec![
            TestCase {
                input: "abc".to_string(),
                expected: Some(HashSet::from_iter(vec![RepositoryNamingRule::MinLengthNotReached {
                    min_length: 4,
                }])),
            },
            TestCase {
                input: "abcd".to_string(),
                expected: None,
            },
            TestCase {
                input: "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabc".to_string(),
                expected: Some(HashSet::from_iter(vec![RepositoryNamingRule::MaxLengthReached {
                    max_length: 54,
                }])),
            },
            TestCase {
                input: "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzab".to_string(),
                expected: None,
            },
            TestCase {
                input: "abc_def_ghi_jkl_mno_pqr_stu_vwx_yz".to_string(),
                expected: Some(HashSet::from_iter(vec![
                    RepositoryNamingRule::AlphaNumericCharsDashesPeriodsOnly,
                ])),
            },
            TestCase {
                input: "a_d".to_string(),
                expected: Some(HashSet::from_iter(vec![
                    RepositoryNamingRule::AlphaNumericCharsDashesPeriodsOnly,
                    RepositoryNamingRule::MinLengthNotReached { min_length: 4 },
                ])),
            },
            TestCase {
                input: "abc_def_ghi_jkl_mno_pqr_stu_vwx_yz@abc_def_ghi_jkl_mno_pqr_stu_vwx_yz".to_string(),
                expected: Some(HashSet::from_iter(vec![
                    RepositoryNamingRule::AlphaNumericCharsDashesPeriodsOnly,
                    RepositoryNamingRule::MaxLengthReached { max_length: 54 },
                ])),
            },
            TestCase {
                input: "abc-def.ghi-jkl.mno-pqr-stu-vwx-yz".to_string(),
                expected: None,
            },
            TestCase {
                input: "abc-def.ghi-jkl.mno-123-stu-vwx-yz".to_string(),
                expected: None,
            },
            TestCase {
                input: "abc-def-ghi-jkl-mno-pqr-stu-vwx-yz".to_string(),
                expected: None,
            },
            TestCase {
                input: "abc.def.ghi.jkl.mno.pqr.stu.vwx.yz".to_string(),
                expected: None,
            },
        ];

        for tc in test_cases {
            // execute:
            let result = ScalewayCR::check_repository_naming_rules(tc.input);

            // verify:
            assert_eq!(tc.expected, result);
        }
    }
}

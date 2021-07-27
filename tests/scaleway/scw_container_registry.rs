extern crate test_utilities;

use self::test_utilities::utilities::{context, FuncTestsSecrets};
use tracing::debug;
use uuid::Uuid;

use qovery_engine::build_platform::Image;
use qovery_engine::cloud_provider::scaleway::application::Region;
use qovery_engine::container_registry::scaleway_container_registry::ScalewayCR;

fn regions_to_test() -> Vec<Region> {
    vec![Region::Paris, Region::Amsterdam, Region::Warsaw]
}

#[cfg(feature = "test-scw-infra")]
#[test]
fn test_push_image() {
    // TODO(benjaminch): Implement
}

#[cfg(feature = "test-scw-infra")]
#[test]
fn test_delete_image() {
    // TODO(benjaminch): Implement
}

#[cfg(feature = "test-scw-infra")]
#[test]
fn test_get_registry_namespace() {
    // setup:
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let scw_secret_key = secrets.SCALEWAY_SECRET_KEY.unwrap_or("undefined".to_string());
    let scw_default_project_id = secrets.SCALEWAY_DEFAULT_PROJECT_ID.unwrap_or("undefined".to_string());

    // testing it in all regions
    for region in regions_to_test().into_iter() {
        let container_registry = ScalewayCR::new(
            context.clone(),
            "".to_string(),
            format!("test-{}-{}", Uuid::new_v4(), &region.to_string()),
            scw_secret_key.to_owned(),
            scw_default_project_id.to_owned(),
            region,
        );

        let image = Image {
            application_id: "1234".to_string(),
            name: "an_image_123".to_string(),
            tag: "tag123".to_string(),
            commit_id: "commit_id".to_string(),
            registry_name: Some(format!("test-{}-{}", Uuid::new_v4(), region.to_string())),
            registry_secret: None,
            registry_url: None,
        };

        container_registry
            .create_registry_namespace(&image)
            .expect("error while creating registry namespace");

        // execute:
        debug!("test_get_registry_namespace - {}", region);
        let result = container_registry.get_registry_namespace(&image);

        // verify:
        assert_eq!(true, result.is_some());

        let result = result.unwrap();
        assert_eq!(true, result.status.is_some());

        let status = result.status.unwrap();
        assert_eq!(
            scaleway_api_rs::models::scaleway_registry_v1_namespace::Status::Ready,
            status,
        );

        // clean-up:
        container_registry.delete_registry_namespace(&image).unwrap();
    }
}

#[cfg(feature = "test-scw-infra")]
#[test]
fn test_create_registry_namespace() {
    // setup:
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let scw_secret_key = secrets.SCALEWAY_SECRET_KEY.unwrap_or("undefined".to_string());
    let scw_default_project_id = secrets.SCALEWAY_DEFAULT_PROJECT_ID.unwrap_or("undefined".to_string());

    // testing it in all regions
    for region in regions_to_test().into_iter() {
        let container_registry = ScalewayCR::new(
            context.clone(),
            "".to_string(),
            format!("test-{}-{}", Uuid::new_v4(), &region.to_string()),
            scw_secret_key.to_owned(),
            scw_default_project_id.to_owned(),
            region,
        );

        let image = Image {
            application_id: "1234".to_string(),
            name: "an_image_123".to_string(),
            tag: "tag123".to_string(),
            commit_id: "commit_id".to_string(),
            registry_name: Some(format!("test-{}-{}", Uuid::new_v4(), &region.to_string())),
            registry_secret: None,
            registry_url: None,
        };

        // execute:
        debug!("test_create_registry_namespace - {}", region);
        let result = container_registry.create_registry_namespace(&image);

        // verify:
        assert_eq!(true, result.is_ok());

        let added_registry_result = container_registry.get_registry_namespace(&image);
        assert_eq!(true, added_registry_result.is_some());

        let added_registry_result = added_registry_result.unwrap();
        assert_eq!(true, added_registry_result.status.is_some());

        // clean-up:
        container_registry.delete_registry_namespace(&image).unwrap();
    }
}

#[cfg(feature = "test-scw-infra")]
#[test]
fn test_delete_registry_namespace() {
    // setup:
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let scw_secret_key = secrets.SCALEWAY_SECRET_KEY.unwrap_or("undefined".to_string());
    let scw_default_project_id = secrets.SCALEWAY_DEFAULT_PROJECT_ID.unwrap_or("undefined".to_string());

    // testing it in all regions
    for region in regions_to_test().into_iter() {
        let container_registry = ScalewayCR::new(
            context.clone(),
            "".to_string(),
            format!("test-{}-{}", Uuid::new_v4(), &region.to_string()),
            scw_secret_key.to_owned(),
            scw_default_project_id.to_owned(),
            region,
        );

        let image = Image {
            application_id: "1234".to_string(),
            name: "an_image_123".to_string(),
            tag: "tag123".to_string(),
            commit_id: "commit_id".to_string(),
            registry_name: Some(format!("test-{}-{}", Uuid::new_v4(), &region.to_string())),
            registry_secret: None,
            registry_url: None,
        };

        container_registry
            .create_registry_namespace(&image)
            .expect("error while creating registry namespace");

        // execute:
        debug!("test_delete_registry_namespace - {}", region);
        let result = container_registry.delete_registry_namespace(&image);

        // verify:
        assert_eq!(true, result.is_ok());
    }
}

#[cfg(feature = "test-scw-infra")]
#[test]
fn test_get_or_create_registry_namespace() {
    // setup:
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let scw_secret_key = secrets.SCALEWAY_SECRET_KEY.unwrap_or("undefined".to_string());
    let scw_default_project_id = secrets.SCALEWAY_DEFAULT_PROJECT_ID.unwrap_or("undefined".to_string());

    // testing it in all regions
    for region in regions_to_test().into_iter() {
        let container_registry = ScalewayCR::new(
            context.clone(),
            "".to_string(),
            format!("test-{}-{}", Uuid::new_v4(), &region.to_string()),
            scw_secret_key.to_owned(),
            scw_default_project_id.to_owned(),
            region,
        );

        let image = Image {
            application_id: "1234".to_string(),
            name: "an_image_123".to_string(),
            tag: "tag123".to_string(),
            commit_id: "commit_id".to_string(),
            registry_name: Some(format!("test-{}-{}", Uuid::new_v4(), &region.to_string())),
            registry_secret: None,
            registry_url: None,
        };

        container_registry
            .create_registry_namespace(&image)
            .expect("error while creating registry namespace");

        // first try: registry not created, should be created

        // execute:
        debug!("test_get_or_create_registry_namespace - {}", region);
        let result = container_registry.get_or_create_registry_namespace(&image);

        // verify:
        assert_eq!(true, result.is_ok());

        let result = result.unwrap();
        assert_eq!(true, result.status.is_some());

        let status = result.status.unwrap();
        assert_eq!(
            scaleway_api_rs::models::scaleway_registry_v1_namespace::Status::Ready,
            status,
        );

        let added_registry_result = container_registry.get_registry_namespace(&image);
        assert_eq!(true, added_registry_result.is_some());

        let added_registry_result = added_registry_result.unwrap();
        assert_eq!(true, added_registry_result.status.is_some());

        // second try: repository already created, so should be a get only
        let result = container_registry.get_or_create_registry_namespace(&image);

        // verify:
        assert_eq!(true, result.is_ok());

        let result = result.unwrap();
        assert_eq!(true, result.status.is_some());

        let status = result.status.unwrap();
        assert_eq!(
            scaleway_api_rs::models::scaleway_registry_v1_namespace::Status::Ready,
            status,
        );

        let added_registry_result = container_registry.get_registry_namespace(&image);
        assert_eq!(true, added_registry_result.is_some());

        let added_registry_result = added_registry_result.unwrap();
        assert_eq!(true, added_registry_result.status.is_some());

        // clean-up:
        container_registry.delete_registry_namespace(&image).unwrap();
    }
}

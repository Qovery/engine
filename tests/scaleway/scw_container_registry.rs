extern crate test_utilities;

use self::test_utilities::utilities::{context, FuncTestsSecrets};
use qovery_engine::cloud_provider::scaleway::application::ScwZone;
use qovery_engine::container_registry::scaleway_container_registry::ScalewayCR;
use test_utilities::utilities::logger;
use tracing::debug;
use uuid::Uuid;

fn zones_to_test() -> Vec<ScwZone> {
    vec![ScwZone::Paris1, ScwZone::Paris2, ScwZone::Amsterdam1, ScwZone::Warsaw1]
}

#[cfg(feature = "test-scw-infra")]
#[ignore] // To be ran only on demand to help with debugging
#[test]
fn test_push_image() {
    // TODO(benjaminch): Implement
}

#[cfg(feature = "test-scw-infra")]
#[ignore] // To be ran only on demand to help with debugging
#[test]
fn test_delete_image() {
    // TODO(benjaminch): Implement
}

#[cfg(feature = "test-scw-infra")]
#[test]
fn test_get_registry_namespace() {
    // setup:
    let context = context("fake_orga_id", "fake_cluster_id");
    let secrets = FuncTestsSecrets::new();
    let scw_secret_key = secrets.SCALEWAY_SECRET_KEY.unwrap_or("undefined".to_string());
    let scw_default_project_id = secrets.SCALEWAY_DEFAULT_PROJECT_ID.unwrap_or("undefined".to_string());

    // testing it in all regions
    for region in zones_to_test().into_iter() {
        let registry_name = format!("test-{}-{}", Uuid::new_v4(), &region.to_string());

        let container_registry = ScalewayCR::new(
            context.clone(),
            "",
            registry_name.as_str(),
            scw_secret_key.as_str(),
            scw_default_project_id.as_str(),
            region,
            logger(),
        );

        let image = registry_name.to_string();
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
    let context = context("fake_orga_id", "fake_cluster_id");
    let secrets = FuncTestsSecrets::new();
    let scw_secret_key = secrets.SCALEWAY_SECRET_KEY.unwrap_or("undefined".to_string());
    let scw_default_project_id = secrets.SCALEWAY_DEFAULT_PROJECT_ID.unwrap_or("undefined".to_string());

    // testing it in all regions
    for region in zones_to_test().into_iter() {
        let registry_name = format!("test-{}-{}", Uuid::new_v4(), &region.to_string());

        let container_registry = ScalewayCR::new(
            context.clone(),
            "",
            registry_name.as_str(),
            scw_secret_key.as_str(),
            scw_default_project_id.as_str(),
            region,
            logger(),
        );

        let image = registry_name.to_string();

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
    let context = context("fake_orga_id", "fake_cluster_id");
    let secrets = FuncTestsSecrets::new();
    let scw_secret_key = secrets.SCALEWAY_SECRET_KEY.unwrap_or("undefined".to_string());
    let scw_default_project_id = secrets.SCALEWAY_DEFAULT_PROJECT_ID.unwrap_or("undefined".to_string());

    // testing it in all regions
    for region in zones_to_test().into_iter() {
        let registry_name = format!("test-{}-{}", Uuid::new_v4(), &region.to_string());

        let container_registry = ScalewayCR::new(
            context.clone(),
            "",
            registry_name.as_str(),
            scw_secret_key.as_str(),
            scw_default_project_id.as_str(),
            region,
            logger(),
        );

        let image = registry_name.to_string();
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
    let context = context("fake_orga_id", "fake_cluster_id");
    let secrets = FuncTestsSecrets::new();
    let scw_secret_key = secrets.SCALEWAY_SECRET_KEY.unwrap_or("undefined".to_string());
    let scw_default_project_id = secrets.SCALEWAY_DEFAULT_PROJECT_ID.unwrap_or("undefined".to_string());

    // testing it in all regions
    for region in zones_to_test().into_iter() {
        let registry_name = format!("test-{}-{}", Uuid::new_v4(), &region.to_string());

        let container_registry = ScalewayCR::new(
            context.clone(),
            "",
            registry_name.as_str(),
            scw_secret_key.as_str(),
            scw_default_project_id.as_str(),
            region,
            logger(),
        );

        let image = registry_name.to_string();
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

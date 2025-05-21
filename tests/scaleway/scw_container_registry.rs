use crate::helpers::scaleway::random_valid_registry_name;
use crate::helpers::utilities::{FuncTestsSecrets, context_for_resource, engine_run_test};
use function_name::named;
use qovery_engine::environment::models::scaleway::ScwZone;
use qovery_engine::infrastructure::models::container_registry::InteractWithRegistry;
use qovery_engine::infrastructure::models::container_registry::errors::{ContainerRegistryError, RepositoryNamingRule};
use qovery_engine::infrastructure::models::container_registry::scaleway_container_registry::ScalewayCR;
use std::collections::HashSet;
use std::iter::FromIterator;
use tracing::debug;
use tracing::{Level, span};
use uuid::Uuid;

fn zones_to_test() -> Vec<ScwZone> {
    vec![ScwZone::Paris1, ScwZone::Paris2, ScwZone::Amsterdam1, ScwZone::Warsaw1]
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[ignore] // To be ran only on demand to help with debugging
#[test]
fn test_push_image() {
    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        // TODO(benjaminch): Implement

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[ignore] // To be ran only on demand to help with debugging
#[test]
fn test_delete_image() {
    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        // TODO(benjaminch): Implement

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn test_get_repository() {
    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();
        // setup:
        let context = context_for_resource(Uuid::new_v4(), Uuid::new_v4());
        let secrets = FuncTestsSecrets::new();
        let scw_secret_key = secrets.SCALEWAY_SECRET_KEY.unwrap_or_else(|| "undefined".to_string());
        let scw_default_project_id = secrets
            .SCALEWAY_DEFAULT_PROJECT_ID
            .unwrap_or_else(|| "undefined".to_string());

        // testing it in all regions
        for zone in zones_to_test().into_iter() {
            let registry_name = format!("test-{}-{}", Uuid::new_v4(), &zone.to_string());

            let container_registry = ScalewayCR::new(
                context.clone(),
                Uuid::new_v4(),
                registry_name.as_str(),
                scw_secret_key.as_str(),
                scw_default_project_id.as_str(),
                zone.region(),
            )
            .unwrap();

            let image = registry_name.to_string();
            container_registry
                .create_registry_namespace(&image)
                .expect("error while creating registry namespace");

            // execute:
            debug!("test_get_repository - {}", zone.region());
            let result = container_registry.get_repository(&image);

            // verify:
            assert!(result.is_ok());

            // clean-up:
            container_registry.delete_repository(&image).unwrap();
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn test_create_registry_namespace() {
    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();
        // setup:
        let context = context_for_resource(Uuid::new_v4(), Uuid::new_v4());
        let secrets = FuncTestsSecrets::new();
        let scw_secret_key = secrets.SCALEWAY_SECRET_KEY.unwrap_or_else(|| "undefined".to_string());
        let scw_default_project_id = secrets
            .SCALEWAY_DEFAULT_PROJECT_ID
            .unwrap_or_else(|| "undefined".to_string());

        // testing it in all regions
        for zone in zones_to_test().into_iter() {
            let registry_name = format!("test-{}-{}", Uuid::new_v4(), &zone.to_string());

            let container_registry = ScalewayCR::new(
                context.clone(),
                Uuid::new_v4(),
                registry_name.as_str(),
                scw_secret_key.as_str(),
                scw_default_project_id.as_str(),
                zone.region(),
            )
            .unwrap();

            let image = registry_name.to_string();

            // execute:
            debug!("test_create_registry_namespace - {}", zone);
            let result = container_registry.create_registry_namespace(&image);

            // verify:
            assert!(result.is_ok());

            let added_registry_result = container_registry.get_repository(&image);
            assert!(added_registry_result.is_ok());

            // clean-up:
            container_registry.delete_repository(&image).unwrap();
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn test_create_registry_namespace_invalid_name() {
    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();
        // setup:
        let context = context_for_resource(Uuid::new_v4(), Uuid::new_v4());
        let secrets = FuncTestsSecrets::new();
        let scw_secret_key = secrets.SCALEWAY_SECRET_KEY.unwrap_or_else(|| "undefined".to_string());
        let scw_default_project_id = secrets
            .SCALEWAY_DEFAULT_PROJECT_ID
            .unwrap_or_else(|| "undefined".to_string());

        struct NamingTestCase {
            name: String,
            expected_error: Option<ContainerRegistryError>,
        }

        // testing it in all regions
        for zone in zones_to_test().into_iter() {
            let registry_name = format!("test-{}-{}", Uuid::new_v4(), &zone.to_string());

            // Very basics tests cases just making sure naming validation is properly plugged
            let naming_test_cases = vec![
                NamingTestCase {
                    name: "abc".to_string(),
                    expected_error: Some(ContainerRegistryError::RepositoryNameNotValid {
                        registry_name: registry_name.to_string(),
                        repository_name: "abc".to_string(),
                        broken_rules: HashSet::from_iter(vec![RepositoryNamingRule::MinLengthNotReached {
                            min_length: 4,
                        }]),
                    }),
                },
                NamingTestCase {
                    name: "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxy".to_string(),
                    expected_error: Some(ContainerRegistryError::RepositoryNameNotValid {
                        registry_name: registry_name.to_string(),
                        repository_name: "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxy".to_string(),
                        broken_rules: HashSet::from_iter(vec![RepositoryNamingRule::MaxLengthReached {
                            max_length: 50,
                        }]),
                    }),
                },
                NamingTestCase {
                    name: "abc_def_ghi_jkl_mno_pqr_stu_vwx_yz@abc_def_ghi_jkl_mno_pqr_stu_vwx_yz".to_string(),
                    expected_error: Some(ContainerRegistryError::RepositoryNameNotValid {
                        registry_name: registry_name.to_string(),
                        repository_name: "abc_def_ghi_jkl_mno_pqr_stu_vwx_yz@abc_def_ghi_jkl_mno_pqr_stu_vwx_yz"
                            .to_string(),
                        broken_rules: HashSet::from_iter(vec![
                            RepositoryNamingRule::AlphaNumericCharsDashesPeriodsOnly,
                            RepositoryNamingRule::MaxLengthReached { max_length: 50 },
                        ]),
                    }),
                },
                NamingTestCase {
                    name: random_valid_registry_name(),
                    expected_error: None,
                },
            ];

            let container_registry = ScalewayCR::new(
                context.clone(),
                Uuid::new_v4(),
                registry_name.as_str(),
                scw_secret_key.as_str(),
                scw_default_project_id.as_str(),
                zone.region(),
            )
            .unwrap();

            for naming_test_case in naming_test_cases {
                let image = naming_test_case.name;

                // execute:
                debug!("test_create_registry_namespace with name {} - {}", image, zone);
                let result = container_registry.get_or_create_registry_namespace(&image);

                // verify:
                match naming_test_case.expected_error {
                    None => {
                        assert!(result.is_ok());

                        let added_registry_result = container_registry.get_repository(&image);
                        assert!(added_registry_result.is_ok());

                        // clean-up:
                        container_registry.delete_repository(&image).unwrap();
                    }
                    Some(e) => {
                        assert_eq!(e, result.unwrap_err());
                    }
                }
            }
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn test_delete_repository() {
    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();
        // setup:
        let context = context_for_resource(Uuid::new_v4(), Uuid::new_v4());
        let secrets = FuncTestsSecrets::new();
        let scw_secret_key = secrets.SCALEWAY_SECRET_KEY.unwrap_or_else(|| "undefined".to_string());
        let scw_default_project_id = secrets
            .SCALEWAY_DEFAULT_PROJECT_ID
            .unwrap_or_else(|| "undefined".to_string());

        // testing it in all regions
        for zone in zones_to_test().into_iter() {
            let registry_name = format!("test-{}-{}", Uuid::new_v4(), &zone.to_string());

            let container_registry = ScalewayCR::new(
                context.clone(),
                Uuid::new_v4(),
                registry_name.as_str(),
                scw_secret_key.as_str(),
                scw_default_project_id.as_str(),
                zone.region(),
            )
            .unwrap();

            let image = registry_name.to_string();
            container_registry
                .create_registry_namespace(&image)
                .expect("error while creating registry namespace");

            // execute:
            debug!("test_delete_repository - {}", zone);
            let result = container_registry.delete_repository(&image);

            // verify:
            assert!(result.is_ok());
        }
        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-minimal")]
#[named]
#[test]
fn test_get_or_create_registry_namespace() {
    let test_name = function_name!();
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();
        // setup:
        let context = context_for_resource(Uuid::new_v4(), Uuid::new_v4());
        let secrets = FuncTestsSecrets::new();
        let scw_secret_key = secrets.SCALEWAY_SECRET_KEY.unwrap_or_else(|| "undefined".to_string());
        let scw_default_project_id = secrets
            .SCALEWAY_DEFAULT_PROJECT_ID
            .unwrap_or_else(|| "undefined".to_string());

        // testing it in all regions
        for zone in zones_to_test().into_iter() {
            let registry_name = format!("test-{}-{}", Uuid::new_v4(), &zone.to_string());

            let container_registry = ScalewayCR::new(
                context.clone(),
                Uuid::new_v4(),
                registry_name.as_str(),
                scw_secret_key.as_str(),
                scw_default_project_id.as_str(),
                zone.region(),
            )
            .unwrap();

            let image = registry_name.to_string();
            container_registry
                .create_registry_namespace(&image)
                .expect("error while creating registry namespace");

            // first try: registry not created, should be created

            // execute:
            debug!("test_get_or_create_registry_namespace - {}", zone);
            let result = container_registry.get_or_create_registry_namespace(&image);

            // verify:
            assert!(result.is_ok());

            let added_registry_result = container_registry.get_repository(&image);
            assert!(added_registry_result.is_ok());

            // second try: repository already created, so should be a get only
            let result = container_registry.get_or_create_registry_namespace(&image);

            // verify:
            assert!(result.is_ok());

            let added_registry_result = container_registry.get_repository(&image);
            assert!(added_registry_result.is_ok());

            // clean-up:
            container_registry.delete_repository(&image).unwrap();
        }
        test_name.to_string()
    })
}

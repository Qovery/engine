use crate::helpers::gcp::{
    try_parse_json_credentials_from_str, GCP_ARTIFACT_REGISTRY_IMAGE_API_OBJECT_WRITE_RATE_LIMITER,
    GCP_ARTIFACT_REGISTRY_REPOSITORY_API_OBJECT_WRITE_RATE_LIMITER, GCP_REGION,
};
use crate::helpers::utilities::{engine_run_test, init, FuncTestsSecrets};
use function_name::named;
use qovery_engine::cmd::command::CommandKiller;
use qovery_engine::cmd::docker::{ContainerImage, Docker};
use qovery_engine::container_registry::{DockerImage, Repository};
use qovery_engine::models::ToCloudProviderFormat;
use qovery_engine::services::gcp::artifact_registry_service::ArtifactRegistryService;
use retry::delay::Fixed;
use retry::OperationResult;
use std::collections::HashMap;
use std::{thread, time::Duration};
use tracing::{error, info, span, Level};
use url::Url;
use uuid::Uuid;

/// Note those tests might be a bit long because of the write limitations on repositories / images

#[cfg(feature = "test-gcp-minimal")]
#[test]
#[named]
fn test_get_repository() {
    // setup:
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let secrets = FuncTestsSecrets::new();
        let gcp_project_name = secrets
            .GCP_PROJECT_NAME
            .as_ref()
            .expect("GCP_PROJECT_NAME should be defined in secrets");
        let credentials = try_parse_json_credentials_from_str(
            secrets
                .GCP_CREDENTIALS
                .as_ref()
                .expect("GCP_CREDENTIALS is not set in secrets"),
        )
        .expect("Cannot parse GCP_CREDENTIALS");
        let service = ArtifactRegistryService::new(
            credentials,
            Some(GCP_ARTIFACT_REGISTRY_REPOSITORY_API_OBJECT_WRITE_RATE_LIMITER.clone()),
            Some(GCP_ARTIFACT_REGISTRY_IMAGE_API_OBJECT_WRITE_RATE_LIMITER.clone()),
        )
        .expect("Cannot initialize google artifact registry service");

        // create a repository for the test
        let repository_parent = format!(
            "projects/{}/locations/{}",
            gcp_project_name,
            GCP_REGION.to_cloud_provider_format()
        );
        let repository_name = format!("test-repository-{}", Uuid::new_v4());
        let _created_repository = service
            .create_repository(
                gcp_project_name,
                GCP_REGION,
                repository_name.as_str(),
                HashMap::from_iter(vec![("test_name".to_string(), function_name!().to_string())]),
            )
            .expect("Cannot create repository");
        // stick a guard on the repository to delete after test
        let _created_repository_name_guard = scopeguard::guard(&repository_name, |repository_name| {
            // make sure to delete the repository after test
            service
                .delete_repository(gcp_project_name, GCP_REGION, repository_name.as_str())
                .unwrap_or_else(|_| panic!("Cannot delete test repository `{}` after test", repository_name));
        });

        // execute:
        let retrieved_repository = service.get_repository(gcp_project_name, GCP_REGION, &repository_name);

        // verify:
        assert!(retrieved_repository.is_ok());
        assert_eq!(
            Repository {
                registry_id: repository_parent.to_string(),
                name: repository_name.to_string(),
                labels: Some(HashMap::from_iter(vec![(
                    "test_name".to_string(),
                    function_name!().to_string()
                )])),
                ttl: None, // TODO(benjaminch): proper TTL should be set
                uri: Some(format!(
                    "{}-docker.pkg.dev/{}/{}/",
                    GCP_REGION.to_cloud_provider_format(),
                    gcp_project_name,
                    repository_name
                ))
            },
            retrieved_repository.expect("Cannot retrieve repository")
        );

        test_name.to_string()
    })
}

#[cfg(feature = "test-gcp-minimal")]
#[test]
#[named]
fn test_create_repository() {
    // setup:
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let secrets = FuncTestsSecrets::new();
        let gcp_project_name = secrets
            .GCP_PROJECT_NAME
            .as_ref()
            .expect("GCP_PROJECT_NAME should be defined in secrets");
        let credentials = try_parse_json_credentials_from_str(
            secrets
                .GCP_CREDENTIALS
                .as_ref()
                .expect("GCP_CREDENTIALS is not set in secrets"),
        )
        .expect("Cannot parse GCP_CREDENTIALS");
        let service = ArtifactRegistryService::new(
            credentials,
            Some(GCP_ARTIFACT_REGISTRY_REPOSITORY_API_OBJECT_WRITE_RATE_LIMITER.clone()),
            Some(GCP_ARTIFACT_REGISTRY_IMAGE_API_OBJECT_WRITE_RATE_LIMITER.clone()),
        )
        .expect("Cannot initialize google artifact registry service");

        let repository_parent = format!(
            "projects/{}/locations/{}",
            gcp_project_name,
            GCP_REGION.to_cloud_provider_format()
        );
        let repository_name = format!("test-repository-{}", Uuid::new_v4());

        // execute:
        let created_repository = service.create_repository(
            gcp_project_name,
            GCP_REGION,
            repository_name.as_str(),
            HashMap::from_iter(vec![("test_name".to_string(), function_name!().to_string())]),
        );

        // stick a guard on the repository to delete after test
        let _created_repository_name_guard = scopeguard::guard(&repository_name, |repository_name| {
            // make sure to delete the repository after test
            service
                .delete_repository(gcp_project_name, GCP_REGION, repository_name.as_str())
                .unwrap_or_else(|_| panic!("Cannot delete test repository `{}` after test", repository_name));
        });

        // verify:
        assert_eq!(
            Ok(Repository {
                registry_id: repository_parent.to_string(),
                name: repository_name.to_string(),
                labels: Some(HashMap::from_iter(vec![(
                    "test_name".to_string(),
                    function_name!().to_string()
                )])),
                ttl: None, // TODO(benjaminch): proper TTL should be set
                uri: Some(format!(
                    "{}-docker.pkg.dev/{}/{}/",
                    GCP_REGION.to_cloud_provider_format(),
                    gcp_project_name,
                    repository_name
                ))
            },),
            created_repository
        );

        test_name.to_string()
    });
}

#[cfg(feature = "test-gcp-minimal")]
#[test]
#[named]
fn test_delete_repository() {
    // setup:
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let secrets = FuncTestsSecrets::new();
        let gcp_project_name = secrets
            .GCP_PROJECT_NAME
            .as_ref()
            .expect("GCP_PROJECT_NAME should be defined in secrets");
        let credentials = try_parse_json_credentials_from_str(
            secrets
                .GCP_CREDENTIALS
                .as_ref()
                .expect("GCP_CREDENTIALS is not set in secrets"),
        )
        .expect("Cannot parse GCP_CREDENTIALS");
        let service = ArtifactRegistryService::new(
            credentials,
            Some(GCP_ARTIFACT_REGISTRY_REPOSITORY_API_OBJECT_WRITE_RATE_LIMITER.clone()),
            Some(GCP_ARTIFACT_REGISTRY_IMAGE_API_OBJECT_WRITE_RATE_LIMITER.clone()),
        )
        .expect("Cannot initialize google artifact registry service");

        // create a repository for the test
        let repository_name = format!("test-repository-{}", Uuid::new_v4());
        let _created_repository = service
            .create_repository(
                gcp_project_name,
                GCP_REGION,
                repository_name.as_str(),
                HashMap::from_iter(vec![("test_name".to_string(), function_name!().to_string())]),
            )
            .expect("Cannot create repository");

        // execute:
        let delete_result = service.delete_repository(gcp_project_name, GCP_REGION, &repository_name);

        // verify:
        assert!(delete_result.is_ok());

        test_name.to_string()
    });
}

#[cfg(feature = "test-quarantine")]
#[test]
// #[ignore = "docker login fails on the CI because of not TTY, TODO(): activate it in the CI"]
#[named]
fn test_get_docker_image() {
    // setup:
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let secrets = FuncTestsSecrets::new();
        let gcp_project_name = secrets
            .GCP_PROJECT_NAME
            .as_ref()
            .expect("GCP_PROJECT_NAME should be defined in secrets");
        let credentials = try_parse_json_credentials_from_str(
            secrets
                .GCP_CREDENTIALS
                .as_ref()
                .expect("GCP_CREDENTIALS is not set in secrets"),
        )
        .expect("Cannot parse GCP_CREDENTIALS");
        let service = ArtifactRegistryService::new(
            credentials,
            Some(GCP_ARTIFACT_REGISTRY_REPOSITORY_API_OBJECT_WRITE_RATE_LIMITER.clone()),
            Some(GCP_ARTIFACT_REGISTRY_IMAGE_API_OBJECT_WRITE_RATE_LIMITER.clone()),
        )
        .expect("Cannot initialize google artifact registry service");

        // create a repository for the test
        let repository_name = format!("test-repository-{}", Uuid::new_v4());
        let _created_repository = service
            .create_repository(
                gcp_project_name,
                GCP_REGION,
                repository_name.as_str(),
                HashMap::from_iter(vec![("test_name".to_string(), function_name!().to_string())]),
            )
            .expect("Cannot create repository");
        // stick a guard on the repository to delete after test
        let _created_repository_name_guard = scopeguard::guard(&repository_name, |repository_name| {
            // make sure to delete the repository after test
            service
                .delete_repository(gcp_project_name, GCP_REGION, repository_name.as_str())
                .unwrap_or_else(|_| panic!("Cannot delete test repository `{}` after test", repository_name));
        });

        // pushing image into the repository
        // https://cloud.google.com/artifact-registry/docs/docker/authentication#json-key
        let docker = Docker::new_with_local_builder(None).expect("Cannot create Docker client");

        let registry_url =
            Url::parse(format!("https://{}-docker.pkg.dev", GCP_REGION.to_cloud_provider_format(),).as_str())
                .expect("Cannot create registry URL");
        let mut repository_url =
            Url::parse(format!("{}/{}/{}", registry_url, gcp_project_name, repository_name.as_str(),).as_str())
                .expect("Cannot create repository URL");
        repository_url
            .set_username("_json_key")
            .expect("Cannot set repository URL username");
        repository_url
            .set_password(secrets.GCP_CREDENTIALS.as_deref())
            .expect("Cannot set repository URL password");

        let source_container_image = ContainerImage::new(
            Url::parse("https://us-docker.pkg.dev").expect("Cannot parse registry Url"),
            "google-samples/containers/gke/hello-app".to_string(),
            vec!["2.0".to_string()],
        );
        let destination_image_name = "hello-app";
        let destination_image_tag = "2.0";
        let destination_container_image = ContainerImage::new(
            Url::parse(format!("https://{}-docker.pkg.dev", GCP_REGION.to_cloud_provider_format(),).as_str())
                .expect("Cannot parse registry Url"),
            format!("{}/{}/{}", gcp_project_name, repository_name, destination_image_name,),
            vec![destination_image_tag.to_string()],
        );

        docker.login(&registry_url).expect("Cannot execute Docker login");
        docker
            .pull(
                &source_container_image,
                &mut |msg| info!("{msg}"),
                &mut |msg| error!("{msg}"),
                &CommandKiller::never(),
            )
            .expect("Cannot pull docker image");
        docker
            .tag(
                &source_container_image,
                &destination_container_image,
                &mut |msg| info!("{msg}"),
                &mut |msg| error!("{msg}"),
                &CommandKiller::never(),
            )
            .expect("Cannot tag docker image");
        docker
            .push(
                &destination_container_image,
                &mut |msg| info!("{msg}"),
                &mut |msg| error!("{msg}"),
                &CommandKiller::never(),
            )
            .expect("Cannot push docker image");

        // execute:
        let retrieved_docker_image = service.get_docker_image(
            gcp_project_name,
            GCP_REGION,
            &repository_name,
            destination_image_name,
            destination_image_tag,
        );

        // verify:
        assert_eq!(
            Ok(DockerImage {
                repository_id: format!(
                    "projects/{}/locations/{}/repositories/{}",
                    gcp_project_name, GCP_REGION, repository_name
                ),
                name: destination_image_name.to_string(),
                tag: destination_image_tag.to_string(),
            }),
            retrieved_docker_image
        );

        test_name.to_string()
    });
}

#[cfg(feature = "test-quarantine")]
#[test]
// #[ignore = "docker login fails on the CI because of not TTY, TODO(): activate it in the CI"]
#[named]
fn test_delete_docker_image() {
    // setup:
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let secrets = FuncTestsSecrets::new();
        let gcp_project_name = secrets
            .GCP_PROJECT_NAME
            .as_ref()
            .expect("GCP_PROJECT_NAME should be defined in secrets");
        let credentials = try_parse_json_credentials_from_str(
            secrets
                .GCP_CREDENTIALS
                .as_ref()
                .expect("GCP_CREDENTIALS is not set in secrets"),
        )
        .expect("Cannot parse GCP_CREDENTIALS");
        let service = ArtifactRegistryService::new(
            credentials,
            Some(GCP_ARTIFACT_REGISTRY_REPOSITORY_API_OBJECT_WRITE_RATE_LIMITER.clone()),
            Some(GCP_ARTIFACT_REGISTRY_IMAGE_API_OBJECT_WRITE_RATE_LIMITER.clone()),
        )
        .expect("Cannot initialize google artifact registry service");

        // create a repository for the test
        let repository_name = format!("test-repository-{}", Uuid::new_v4());
        let _created_repository = service
            .create_repository(
                gcp_project_name,
                GCP_REGION,
                repository_name.as_str(),
                HashMap::from_iter(vec![("test_name".to_string(), function_name!().to_string())]),
            )
            .expect("Cannot create repository");
        // stick a guard on the repository to delete after test
        let _created_repository_name_guard = scopeguard::guard(&repository_name, |repository_name| {
            // make sure to delete the repository after test
            service
                .delete_repository(gcp_project_name, GCP_REGION, repository_name.as_str())
                .unwrap_or_else(|_| panic!("Cannot delete test repository `{}` after test", repository_name));
        });

        // pushing image into the repository
        // https://cloud.google.com/artifact-registry/docs/docker/authentication#json-key
        let docker = Docker::new_with_local_builder(None).expect("Cannot create Docker client");

        let registry_url =
            Url::parse(format!("https://{}-docker.pkg.dev", GCP_REGION.to_cloud_provider_format(),).as_str())
                .expect("Cannot create registry URL");
        let mut repository_url =
            Url::parse(format!("{}/{}/{}", registry_url, gcp_project_name, repository_name.as_str(),).as_str())
                .expect("Cannot create repository URL");
        repository_url
            .set_username("_json_key")
            .expect("Cannot set repository URL username");
        repository_url
            .set_password(secrets.GCP_CREDENTIALS.as_deref())
            .expect("Cannot set repository URL password");

        let source_container_image = ContainerImage::new(
            Url::parse("https://us-docker.pkg.dev").expect("Cannot parse registry Url"),
            "google-samples/containers/gke/hello-app".to_string(),
            vec!["2.0".to_string()],
        );
        let destination_image_name = "hello-app";
        let destination_image_tag = "2.0";
        let destination_container_image = ContainerImage::new(
            Url::parse(format!("https://{}-docker.pkg.dev", GCP_REGION.to_cloud_provider_format(),).as_str())
                .expect("Cannot parse registry Url"),
            format!("{}/{}/{}", gcp_project_name, repository_name, destination_image_name,),
            vec![destination_image_tag.to_string()],
        );

        docker.login(&registry_url).expect("Cannot execute Docker login");
        docker
            .pull(
                &source_container_image,
                &mut |msg| info!("{msg}"),
                &mut |msg| error!("{msg}"),
                &CommandKiller::never(),
            )
            .expect("Cannot pull docker image");
        docker
            .tag(
                &source_container_image,
                &destination_container_image,
                &mut |msg| info!("{msg}"),
                &mut |msg| error!("{msg}"),
                &CommandKiller::never(),
            )
            .expect("Cannot tag docker image");
        docker
            .push(
                &destination_container_image,
                &mut |msg| info!("{msg}"),
                &mut |msg| error!("{msg}"),
                &CommandKiller::never(),
            )
            .expect("Cannot push docker image");

        // Wait a bit to let image upload be taken into account
        thread::sleep(Duration::from_millis(5000));

        // execute:
        let delete_docker_image_result =
            service.delete_docker_image(gcp_project_name, GCP_REGION, &repository_name, destination_image_name);

        // verify:
        assert!(delete_docker_image_result.is_ok());

        // there might be a little lag for provider to reflect the deletion, so stick a retry
        let get_image_res = retry::retry(Fixed::from_millis(5000).take(3), || {
            match service.get_docker_image(
                gcp_project_name,
                GCP_REGION,
                &repository_name,
                destination_image_name,
                destination_image_tag,
            ) {
                Ok(_) => OperationResult::Err(()), // Image exists
                Err(_) => OperationResult::Ok(()), // Image doesn't exist
            }
        });
        assert!(get_image_res.is_ok()); // Image doesn't exist

        test_name.to_string()
    })
}

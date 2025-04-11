use crate::helpers::azure::{AZURE_CONTAINER_REGISTRY_SKU, AZURE_LOCATION, AZURE_RESOURCE_GROUP_NAME};
use crate::helpers::utilities::{FuncTestsSecrets, engine_run_test, init};
use function_name::named;
use qovery_engine::cmd::command::CommandKiller;
use qovery_engine::cmd::docker::{ContainerImage, Docker};
use qovery_engine::infrastructure::models::container_registry::{DockerImage, Repository};
use qovery_engine::io_models::QoveryIdentifier;
use qovery_engine::services::azure::container_registry_service::AzureContainerRegistryService;
use tracing::{Level, error, info, span};

#[cfg(feature = "test-azure-minimal")]
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
        let azure_subscription_id = secrets
            .AZURE_SUBSCRIPTION_ID
            .as_ref()
            .expect("AZURE_SUBSCRIPTION_ID should be defined in secrets");
        let azure_tenant_id = secrets
            .AZURE_TENANT_ID
            .as_ref()
            .expect("AZURE_TENANT_ID should be defined in secrets");
        let azure_client_id = secrets
            .AZURE_CLIENT_ID
            .as_ref()
            .expect("AZURE_CLIENT_ID should be defined in secrets");
        let azure_client_secret = secrets
            .AZURE_CLIENT_SECRET
            .as_ref()
            .expect("AZURE_CLIENT_SECRET should be defined in secrets");

        let service = AzureContainerRegistryService::new(azure_tenant_id, azure_client_id, azure_client_secret)
            .expect("Cannot initialize azure container registry service");

        // create a repository for the test
        let repository_name = format!("testrepository{}", QoveryIdentifier::new_random().short());
        let created_repository = service
            .create_registry(
                azure_subscription_id.as_str(),
                AZURE_RESOURCE_GROUP_NAME,
                AZURE_LOCATION,
                repository_name.as_str(),
                AZURE_CONTAINER_REGISTRY_SKU.to_owned(),
                None,
                None,
            )
            .expect("Cannot create repository");
        // stick a guard on the repository to delete after test
        let _created_repository_name_guard = scopeguard::guard(&repository_name, |repository_name| {
            // make sure to delete the repository after test
            service
                .delete_registry(
                    azure_subscription_id.as_str(),
                    AZURE_RESOURCE_GROUP_NAME,
                    repository_name.as_str(),
                )
                .unwrap_or_else(|_| panic!("Cannot delete test repository `{}` after test", repository_name));
        });

        // execute:
        let retrieved_repository_result = service.get_registry(
            azure_subscription_id.as_str(),
            AZURE_RESOURCE_GROUP_NAME,
            repository_name.as_str(),
        );

        // verify:
        assert!(retrieved_repository_result.is_ok());
        let retrieved_repository = retrieved_repository_result.expect("Cannot retrieve repository");
        assert_eq!(
            Repository {
                registry_id: created_repository.registry_id,
                name: created_repository.name,
                uri: created_repository.uri,
                ttl: created_repository.ttl,
                labels: created_repository.labels,
            },
            retrieved_repository
        );

        test_name.to_string()
    })
}

#[cfg(feature = "test-azure-minimal")]
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
        let azure_subscription_id = secrets
            .AZURE_SUBSCRIPTION_ID
            .as_ref()
            .expect("AZURE_SUBSCRIPTION_ID should be defined in secrets");
        let azure_tenant_id = secrets
            .AZURE_TENANT_ID
            .as_ref()
            .expect("AZURE_TENANT_ID should be defined in secrets");
        let azure_client_id = secrets
            .AZURE_CLIENT_ID
            .as_ref()
            .expect("AZURE_CLIENT_ID should be defined in secrets");
        let azure_client_secret = secrets
            .AZURE_CLIENT_SECRET
            .as_ref()
            .expect("AZURE_CLIENT_SECRET should be defined in secrets");

        let service = AzureContainerRegistryService::new(azure_tenant_id, azure_client_id, azure_client_secret)
            .expect("Cannot initialize azure container registry service");

        // execute:
        let repository_name = format!("testrepository{}", QoveryIdentifier::new_random().short());
        let created_repository_result = service.create_registry(
            azure_subscription_id.as_str(),
            AZURE_RESOURCE_GROUP_NAME,
            AZURE_LOCATION,
            repository_name.as_str(),
            AZURE_CONTAINER_REGISTRY_SKU.to_owned(),
            None,
            None,
        );
        // stick a guard on the repository to delete after test
        let _created_repository_name_guard = scopeguard::guard(repository_name.clone(), |repository_name| {
            // make sure to delete the repository after test
            service
                .delete_registry(
                    azure_subscription_id.as_str(),
                    AZURE_RESOURCE_GROUP_NAME,
                    repository_name.as_str(),
                )
                .unwrap_or_else(|_| panic!("Cannot delete test repository `{}` after test", repository_name));
        });

        // verify:
        assert!(created_repository_result.is_ok());
        let created_repository = created_repository_result.expect("Cannot create repository");
        assert_eq!(
            Repository {
                registry_id: created_repository.registry_id.to_string(), // no need to check exact value
                name: repository_name.to_string(),
                uri: created_repository.uri.clone(), // no need to check exact value
                ttl: created_repository.ttl,
                labels: created_repository.labels.clone(),
            },
            created_repository
        );

        test_name.to_string()
    })
}

#[cfg(feature = "test-azure-minimal")]
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
        let azure_subscription_id = secrets
            .AZURE_SUBSCRIPTION_ID
            .as_ref()
            .expect("AZURE_SUBSCRIPTION_ID should be defined in secrets");
        let azure_tenant_id = secrets
            .AZURE_TENANT_ID
            .as_ref()
            .expect("AZURE_TENANT_ID should be defined in secrets");
        let azure_client_id = secrets
            .AZURE_CLIENT_ID
            .as_ref()
            .expect("AZURE_CLIENT_ID should be defined in secrets");
        let azure_client_secret = secrets
            .AZURE_CLIENT_SECRET
            .as_ref()
            .expect("AZURE_CLIENT_SECRET should be defined in secrets");

        let service = AzureContainerRegistryService::new(azure_tenant_id, azure_client_id, azure_client_secret)
            .expect("Cannot initialize azure container registry service");

        // create a repository for the test
        let repository_name = format!("testrepository{}", QoveryIdentifier::new_random().short());
        let _created_repository = service
            .create_registry(
                azure_subscription_id.as_str(),
                AZURE_RESOURCE_GROUP_NAME,
                AZURE_LOCATION,
                repository_name.as_str(),
                AZURE_CONTAINER_REGISTRY_SKU.to_owned(),
                None,
                None,
            )
            .expect("Cannot create repository");
        // stick a guard on the repository to delete after test
        let _created_repository_name_guard = scopeguard::guard(&repository_name, |repository_name| {
            // make sure to delete the repository after test
            service
                .delete_registry(
                    azure_subscription_id.as_str(),
                    AZURE_RESOURCE_GROUP_NAME,
                    repository_name.as_str(),
                )
                .unwrap_or_else(|_| panic!("Cannot delete test repository `{}` after test", repository_name));
        });

        // execute:
        let deleted_repository_result = service.delete_registry(
            azure_subscription_id.as_str(),
            AZURE_RESOURCE_GROUP_NAME,
            repository_name.as_str(),
        );

        // verify:
        assert!(deleted_repository_result.is_ok());

        test_name.to_string()
    })
}

#[cfg(feature = "test-azure-minimal")]
#[test]
#[named]
fn test_get_docker_image() {
    // setup:
    use url::Url;
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let secrets = FuncTestsSecrets::new();
        let azure_subscription_id = secrets
            .AZURE_SUBSCRIPTION_ID
            .as_ref()
            .expect("AZURE_SUBSCRIPTION_ID should be defined in secrets");
        let azure_tenant_id = secrets
            .AZURE_TENANT_ID
            .as_ref()
            .expect("AZURE_TENANT_ID should be defined in secrets");
        let azure_client_id = secrets
            .AZURE_CLIENT_ID
            .as_ref()
            .expect("AZURE_CLIENT_ID should be defined in secrets");
        let azure_client_secret = secrets
            .AZURE_CLIENT_SECRET
            .as_ref()
            .expect("AZURE_CLIENT_SECRET should be defined in secrets");

        let service = AzureContainerRegistryService::new(azure_tenant_id, azure_client_id, azure_client_secret)
            .expect("Cannot initialize azure container registry service");

        // create a repository for the test
        let repository_name = format!("testrepository{}", QoveryIdentifier::new_random().short());
        let _created_repository = service
            .create_registry(
                azure_subscription_id.as_str(),
                AZURE_RESOURCE_GROUP_NAME,
                AZURE_LOCATION,
                repository_name.as_str(),
                AZURE_CONTAINER_REGISTRY_SKU.to_owned(),
                None,
                None,
            )
            .expect("Cannot create repository");
        // stick a guard on the repository to delete after test
        let _created_repository_name_guard = scopeguard::guard(&repository_name, |repository_name| {
            // make sure to delete the repository after test
            service
                .delete_registry(
                    azure_subscription_id.as_str(),
                    AZURE_RESOURCE_GROUP_NAME,
                    repository_name.as_str(),
                )
                .unwrap_or_else(|_| panic!("Cannot delete test repository `{}` after test", repository_name));
        });

        // pushing image into the repository
        let docker = Docker::new_with_local_builder(None).expect("Cannot create Docker client");

        let mut repository_url = Url::parse(format!("https://{}.azurecr.io", repository_name.as_str(),).as_str())
            .expect("Cannot create repository URL");
        repository_url
            .set_username(azure_client_id.as_str())
            .expect("Cannot set repository URL username");
        repository_url
            .set_password(Some(azure_client_secret.as_str()))
            .expect("Cannot set repository URL password");

        let source_container_image = ContainerImage::new(
            Url::parse("https://mcr.microsoft.com").expect("Cannot parse registry Url"),
            "hello-world".to_string(),
            vec!["latest".to_string()],
        );
        let destination_image_name = "hello-world";
        let destination_image_tag = "v1";
        let destination_container_image = ContainerImage::new(
            Url::parse(format!("https://{}.azurecr.io", repository_name).as_str()).expect("Cannot parse registry Url"),
            destination_image_name.to_string(),
            vec![destination_image_tag.to_string()],
        );

        docker.login(&repository_url).expect("Cannot execute Docker login");
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
        let retrieved_image = service
            .get_docker_image(
                azure_subscription_id.as_str(),
                AZURE_RESOURCE_GROUP_NAME,
                repository_name.as_str(),
                destination_image_name,
                destination_image_tag,
            )
            .expect("Cannot retrieve docker image");

        // verify:
        assert_eq!(
            DockerImage {
                repository_id: repository_name.to_string(),
                name: destination_image_name.to_string(),
                tag: destination_image_tag.to_string(),
            },
            retrieved_image
        );

        test_name.to_string()
    })
}

#[cfg(feature = "test-azure-minimal")]
#[test]
#[named]
fn test_list_docker_images() {
    // setup:
    use url::Url;
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let secrets = FuncTestsSecrets::new();
        let azure_subscription_id = secrets
            .AZURE_SUBSCRIPTION_ID
            .as_ref()
            .expect("AZURE_SUBSCRIPTION_ID should be defined in secrets");
        let azure_tenant_id = secrets
            .AZURE_TENANT_ID
            .as_ref()
            .expect("AZURE_TENANT_ID should be defined in secrets");
        let azure_client_id = secrets
            .AZURE_CLIENT_ID
            .as_ref()
            .expect("AZURE_CLIENT_ID should be defined in secrets");
        let azure_client_secret = secrets
            .AZURE_CLIENT_SECRET
            .as_ref()
            .expect("AZURE_CLIENT_SECRET should be defined in secrets");

        let service = AzureContainerRegistryService::new(azure_tenant_id, azure_client_id, azure_client_secret)
            .expect("Cannot initialize azure container registry service");

        // create a repository for the test
        let repository_name = format!("testrepository{}", QoveryIdentifier::new_random().short());
        let _created_repository = service
            .create_registry(
                azure_subscription_id.as_str(),
                AZURE_RESOURCE_GROUP_NAME,
                AZURE_LOCATION,
                repository_name.as_str(),
                AZURE_CONTAINER_REGISTRY_SKU.to_owned(),
                None,
                None,
            )
            .expect("Cannot create repository");
        // stick a guard on the repository to delete after test
        let _created_repository_name_guard = scopeguard::guard(&repository_name, |repository_name| {
            // make sure to delete the repository after test
            service
                .delete_registry(
                    azure_subscription_id.as_str(),
                    AZURE_RESOURCE_GROUP_NAME,
                    repository_name.as_str(),
                )
                .unwrap_or_else(|_| panic!("Cannot delete test repository `{}` after test", repository_name));
        });

        // pushing image into the repository
        let docker = Docker::new_with_local_builder(None).expect("Cannot create Docker client");

        let mut repository_url = Url::parse(format!("https://{}.azurecr.io", repository_name.as_str(),).as_str())
            .expect("Cannot create repository URL");
        repository_url
            .set_username(azure_client_id.as_str())
            .expect("Cannot set repository URL username");
        repository_url
            .set_password(Some(azure_client_secret.as_str()))
            .expect("Cannot set repository URL password");

        let source_container_image = ContainerImage::new(
            Url::parse("https://mcr.microsoft.com").expect("Cannot parse registry Url"),
            "hello-world".to_string(),
            vec!["latest".to_string()],
        );
        let destination_image_name = "hello-world";
        let destination_image_tag = "v1";
        let destination_container_image = ContainerImage::new(
            Url::parse(format!("https://{}.azurecr.io", repository_name).as_str()).expect("Cannot parse registry Url"),
            destination_image_name.to_string(),
            vec![destination_image_tag.to_string()],
        );

        docker.login(&repository_url).expect("Cannot execute Docker login");
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
        let retrieved_images = service
            .list_docker_images(
                azure_subscription_id.as_str(),
                AZURE_RESOURCE_GROUP_NAME,
                repository_name.as_str(),
            )
            .expect("Cannot list docker images");

        // verify:
        assert_eq!(
            vec![DockerImage {
                repository_id: repository_name.to_string(),
                name: destination_image_name.to_string(),
                tag: "".to_string(),
            }],
            retrieved_images
        );

        test_name.to_string()
    })
}

use crate::helpers::utilities::{FuncTestsSecrets, context_for_resource, engine_run_test, generate_id, init};
use function_name::named;
use qovery_engine::cmd::command::CommandKiller;
use qovery_engine::cmd::docker::ContainerImage;
use qovery_engine::infrastructure::models::build_platform::Image;
use qovery_engine::infrastructure::models::container_registry::InteractWithRegistry;
use qovery_engine::infrastructure::models::container_registry::github_cr::GithubCr;
use tracing::{Level, span};
use url::Url;
use uuid::Uuid;

#[cfg(feature = "test-local-docker")]
#[named]
#[test]
fn test_github_cr() {
    use qovery_engine::infrastructure::models::container_registry::RegistryTags;

    let test_name = function_name!();
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let context = context_for_resource(generate_id(), generate_id());
        let secrets = FuncTestsSecrets::new();
        let registry_name = format!("test-{}", Uuid::new_v4());
        let container_registry = GithubCr::new(
            context.clone(),
            Uuid::new_v4(),
            registry_name.as_str(),
            Url::parse("https://ghcr.io").unwrap(),
            "qovery".to_string(),
            secrets.GITHUB_ACCESS_TOKEN.unwrap(),
        )
        .unwrap();

        let img_name = Uuid::new_v4();
        let repo_name = container_registry
            .registry_info()
            .get_repository_name(img_name.to_string().as_str());
        let repo_creation = container_registry.create_repository(
            Some(registry_name.as_str()),
            repo_name.as_str(),
            0,
            RegistryTags {
                cluster_id: None,
                environment_id: Some(Uuid::new_v4().to_string()),
                project_id: Some(Uuid::new_v4().to_string()),
                resource_ttl: None,
            },
        );
        assert!(repo_creation.is_ok());

        // given
        let source_img = ContainerImage::new(
            Url::parse("https://public.ecr.aws/").unwrap(),
            "r3m4q3r9/qovery-ci".to_string(),
            vec!["pause-3.10".to_string()],
        );
        let dest_img = ContainerImage::new(
            container_registry.registry_info().registry_endpoint.clone(),
            container_registry
                .registry_info()
                .get_image_name(img_name.to_string().as_str()),
            vec!["test".to_string()],
        );
        context
            .docker
            .mirror(&source_img, &dest_img, &mut |_| {}, &mut |_| {}, &CommandKiller::never())
            .unwrap();
        let source_img = ContainerImage::new(
            Url::parse("https://public.ecr.aws/").unwrap(),
            "r3m4q3r9/qovery-ci".to_string(),
            vec!["debian-bookworm-slim".to_string()],
        );
        let dest_img = ContainerImage::new(
            container_registry.registry_info().registry_endpoint.clone(),
            container_registry
                .registry_info()
                .get_image_name(img_name.to_string().as_str()),
            vec!["test2".to_string()],
        );
        context
            .docker
            .mirror(&source_img, &dest_img, &mut |_| {}, &mut |_| {}, &CommandKiller::never())
            .unwrap();

        // then
        let image = Image {
            name: container_registry
                .registry_info()
                .get_image_name(img_name.to_string().as_str()),
            repository_name: "qovery".to_string(),
            tag: "test".to_string(),
            registry_name: registry_name.clone(),
            registry_url: Url::parse("https://ghcr.io").unwrap(),
            ..Default::default()
        };
        let image2 = Image {
            name: container_registry
                .registry_info()
                .get_image_name(img_name.to_string().as_str()),
            repository_name: "qovery".to_string(),
            tag: "test2".to_string(),
            registry_name,
            registry_url: Url::parse("https://ghcr.io").unwrap(),
            ..Default::default()
        };
        assert!(container_registry.image_exists(&image));
        assert!(container_registry.image_exists(&image2));

        container_registry.delete_image(&image).unwrap();
        assert!(!container_registry.image_exists(&image));
        assert!(container_registry.image_exists(&image2));

        container_registry.delete_image(&image2).unwrap();
        assert!(!container_registry.image_exists(&image2));

        container_registry.delete_repository(&image.name).unwrap();
        assert!(!container_registry.image_exists(&image));
        assert!(!container_registry.image_exists(&image2));

        test_name.to_string()
    })
}

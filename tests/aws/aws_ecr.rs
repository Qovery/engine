use crate::helpers::aws::AWS_QUICK_RESOURCE_TTL_IN_SECONDS;
use crate::helpers::utilities::{context_for_resource, engine_run_test, generate_id, init, logger, FuncTestsSecrets};
use function_name::named;
use qovery_engine::container_registry::ecr::ECR;
use qovery_engine::container_registry::ContainerRegistry;
use qovery_engine::runtime::block_on;
use rusoto_ecr::Ecr;
use rusoto_ecr::{DescribeRepositoriesRequest, ListTagsForResourceRequest, Tag};
use std::time::Duration;
use tracing::{span, Level};
use uuid::Uuid;

#[cfg(feature = "test-aws-minimal")]
#[named]
#[test]
fn create_ecr_repository_with_tags() {
    use qovery_engine::container_registry::RegistryTags;

    let test_name = function_name!();
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let context = context_for_resource(generate_id(), generate_id());
        let secrets = FuncTestsSecrets::new();
        let registry_name = format!("test-{}", Uuid::new_v4());
        let container_registry = ECR::new(
            context,
            "",
            Uuid::new_v4(),
            registry_name.as_str(),
            &secrets.AWS_ACCESS_KEY_ID.expect("Unable to get access key"),
            &secrets.AWS_SECRET_ACCESS_KEY.expect("Unable to get secret key"),
            &secrets.AWS_DEFAULT_REGION.expect("Unable to get default region"),
            logger(),
            hashmap! {"ttl".to_string() => AWS_QUICK_RESOURCE_TTL_IN_SECONDS.to_string()},
        )
        .unwrap();

        let cr = container_registry.create_registry();
        assert!(cr.is_ok());

        let repo_name = format!("test-{}", Uuid::new_v4());
        let repo_creation = container_registry.create_repository(
            repo_name.as_str(),
            AWS_QUICK_RESOURCE_TTL_IN_SECONDS,
            RegistryTags {
                environment_id: Uuid::new_v4().to_string(),
                project_id: Uuid::new_v4().to_string(),
                resource_ttl: Some(Duration::from_secs(AWS_QUICK_RESOURCE_TTL_IN_SECONDS as u64)),
            },
        );
        assert!(repo_creation.is_ok());

        let result = block_on(
            container_registry
                .ecr_client()
                .describe_repositories(DescribeRepositoriesRequest {
                    max_results: None,
                    next_token: None,
                    registry_id: None,
                    repository_names: Some(vec![repo_name]),
                }),
        );
        assert!(result.is_ok());

        if let Ok(response) = result {
            if let Some(repos) = response.repositories {
                if let Some(arn) = &repos[0].repository_arn {
                    let result = block_on(container_registry.ecr_client().list_tags_for_resource(
                        ListTagsForResourceRequest {
                            resource_arn: arn.to_string(),
                        },
                    ));
                    assert!(result.is_ok());
                    if let Ok(response) = result {
                        if let Some(tags) = response.tags {
                            assert!(tags.contains(&Tag {
                                key: Some("ttl".to_string()),
                                value: Some(AWS_QUICK_RESOURCE_TTL_IN_SECONDS.to_string())
                            }))
                        }
                    }
                }
            }
        }

        test_name.to_string()
    })
}

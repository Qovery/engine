use crate::helpers::utilities::{context, generate_id, logger, FuncTestsSecrets};
use qovery_engine::container_registry::ecr::ECR;
use qovery_engine::container_registry::ContainerRegistry;
use qovery_engine::io_models::progress_listener::NoOpProgressListener;
use qovery_engine::runtime::block_on;
use rusoto_ecr::Ecr;
use rusoto_ecr::{DescribeRepositoriesRequest, ListTagsForResourceRequest, Tag};
use std::sync::Arc;
use uuid::Uuid;

#[cfg(feature = "test-aws-infra")]
#[test]
fn create_ecr_repository_with_tags() {
    let context = context(generate_id(), generate_id());
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
        Arc::new(Box::new(NoOpProgressListener {})),
        logger(),
        hashmap! {"ttl".to_string() => 3600.to_string()},
    )
    .unwrap();

    let cr = container_registry.create_registry();
    assert!(cr.is_ok());

    let repo_name = format!("test-{}", Uuid::new_v4());
    let repo_creation = container_registry.create_repository(repo_name.as_str(), 3600);
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
                            value: Some(3600.to_string())
                        }))
                    }
                }
            }
        }
    }
}

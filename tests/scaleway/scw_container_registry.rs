extern crate test_utilities;

use self::test_utilities::cloudflare::dns_provider_cloudflare;
use self::test_utilities::utilities::{
    context, engine_run_test, generate_cluster_id, generate_id, init, FuncTestsSecrets,
};
use std::env;
use test_utilities::aws::AWS_KUBERNETES_VERSION;
use tracing::{span, Level};
use uuid::Uuid;

use qovery_engine::build_platform::Image;
use qovery_engine::cloud_provider::scaleway::application::Region;
use qovery_engine::container_registry::scaleway_cr::ScalewayCR;
use qovery_engine::container_registry::ContainerRegistry;
use qovery_engine::transaction::TransactionResult;

#[test]
fn test_create_registry_namespace() {
    // setup:
    let context = context();
    let secrets = FuncTestsSecrets::new();

    let container_registry = ScalewayCR::new(
        context,
        "".to_string(),
        format!("test-{}", Uuid::new_v4()),
        secrets.SCALEWAY_SECRET_TOKEN.unwrap_or("undefined".to_string()),
        secrets.SCALEWAY_DEFAULT_PROJECT_ID.unwrap_or("undefined".to_string()),
        Region::Paris, // TODO(benjaminch): maybe change the default region or make it customizable
    );

    let image = Image {
        application_id: "1234".to_string(),
        name: "not_existing_image".to_string(),
        tag: "tag123".to_string(),
        commit_id: "commit_id".to_string(),
        registry_name: Some(format!("test-{}", Uuid::new_v4())),
        registry_secret: None,
        registry_url: None,
    };

    // execute:
    let result = container_registry.create_registry_namespace(&image);

    // verify:
    assert_eq!(true, result.is_ok());

    // clean-up:
    container_registry.delete_registry_namespace(&image).unwrap();
}

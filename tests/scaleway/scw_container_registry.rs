extern crate test_utilities;

use self::test_utilities::cloudflare::dns_provider_cloudflare;
use self::test_utilities::utilities::{context, engine_run_test, generate_id, generate_cluster_id, init, FuncTestsSecrets};
use std::env;
use test_utilities::aws::AWS_KUBERNETES_VERSION;
use tracing::{span, Level};

use qovery_engine::transaction::TransactionResult;
use qovery_engine::container_registry::scaleway_cr::ScalewayCR;
use qovery_engine::cloud_provider::scaleway::application::Region;
use qovery_engine::container_registry::ContainerRegistry;
use qovery_engine::build_platform::Image;

#[test]
fn check_if_image_exist() {
    // setup:
    let context = context();
    let secrets = FuncTestsSecrets::new();

    let container_registry = ScalewayCR::new(
        context,
        "test".to_string(),
        "test".to_string(),
        secrets.SCALEWAY_SECRET_TOKEN.unwrap_or("undefined".to_string()),
        secrets.SCALEWAY_DEFAULT_PROJECT_ID.unwrap_or("undefined".to_string()),
        Region::Paris, // TODO(benjaminch): maybe change the default region or make it customizable
    );

    let image = Image{
        application_id: "1234".to_string(),
        name: "test".to_string(),
        tag: "tag123".to_string(),
        commit_id: "commit_id".to_string(),
        registry_name: None,
        registry_secret: None,
        registry_url: None,
    };

    // execute:
    let result = container_registry.does_image_exists(&image);

    // verify:
    assert_eq!(false, result);
}



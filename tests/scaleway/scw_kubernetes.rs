extern crate test_utilities;

use self::test_utilities::cloudflare::dns_provider_cloudflare;
use self::test_utilities::utilities::{
    context, engine_run_test, generate_cluster_id, generate_id, init, FuncTestsSecrets,
};
use std::env;
use tracing::{span, Level};

use qovery_engine::cloud_provider::kubernetes::Kind::ScwKapsule;
use qovery_engine::cloud_provider::scaleway::application::Region;
use qovery_engine::cloud_provider::scaleway::kubernetes::Kapsule;
use qovery_engine::transaction::TransactionResult;
use std::str::FromStr;

#[test]
fn create_upgrade_and_destroy_kapsule_cluster_in_fr_par() {
    let region = Region::Paris;
    let secrets = FuncTestsSecrets::new();
    create_upgrade_and_destroy_kapsule_cluster(
        region,
        secrets,
        "1.20",
        "1.21",
        &format!(
            "create_upgrade_and_destroy_kapsule_cluster_in_{}",
            region.as_str().replace("-", "_")
        ),
    );
}

fn create_upgrade_and_destroy_kapsule_cluster(
    region: Region,
    secrets: FuncTestsSecrets,
    boot_version: &str,
    upgrade_to_version: &str,
    test_name: &str,
) {
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let context = context();
        let engine = test_utilities::scaleway::docker_scw_cr_engine(&context);
        let session = engine.session().unwrap();
        let mut tx = session.transaction();

        let scw_cluster = test_utilities::scaleway::cloud_provider_scaleway(&context);
        let nodes = test_utilities::scaleway::scw_kubernetes_nodes();

        let object_storage = test_utilities::scaleway::scw_object_storage(context.clone(), region);

        let cloudflare = dns_provider_cloudflare(&context);

        let kubernetes = Kapsule::new(
            context.clone(),
            generate_cluster_id(region.as_str()),
            generate_cluster_id(region.as_str()),
            boot_version.to_string(),
            region,
            &scw_cluster,
            &cloudflare,
            object_storage,
            nodes,
            test_utilities::scaleway::scw_kubernetes_cluster_options(secrets),
        );

        // Deploy
        if let Err(err) = tx.create_kubernetes(&kubernetes) {
            panic!("{:?}", err)
        }
        let _ = match tx.commit() {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        // Upgrade
        // TODO(benjaminch): To be added

        // Destroy
        if let Err(err) = tx.delete_kubernetes(&kubernetes) {
            panic!("{:?}", err)
        }
        match tx.commit() {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        test_name.to_string()
    });
}

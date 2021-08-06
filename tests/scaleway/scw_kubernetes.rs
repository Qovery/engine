extern crate test_utilities;

use self::test_utilities::cloudflare::dns_provider_cloudflare;
use self::test_utilities::utilities::{context, engine_run_test, generate_cluster_id, init, FuncTestsSecrets};
use tracing::{span, Level};

use qovery_engine::cloud_provider::scaleway::application::Region;
use qovery_engine::cloud_provider::scaleway::kubernetes::Kapsule;
use qovery_engine::transaction::TransactionResult;

use test_utilities::scaleway::SCW_KUBERNETES_VERSION;

#[allow(dead_code)]
fn create_upgrade_and_destroy_kapsule_cluster(
    region: Region,
    secrets: FuncTestsSecrets,
    boot_version: &str,
    _upgrade_to_version: &str,
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
        let cloudflare = dns_provider_cloudflare(&context);

        let cluster_id = format!("qovery-test-{}", generate_cluster_id(region.as_str()));

        let kubernetes = Kapsule::new(
            context,
            cluster_id.clone(),
            cluster_id,
            boot_version.to_string(),
            region,
            &scw_cluster,
            &cloudflare,
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
        //let kubernetes = ...
        // if let Err(err) = tx.create_kubernetes(&kubernetes) {
        //     panic!("{:?}", err)
        // }
        // let _ = match tx.commit() {
        //     TransactionResult::Ok => assert!(true),
        //     TransactionResult::Rollback(_) => assert!(false),
        //     TransactionResult::UnrecoverableError(_, _) => assert!(false),
        // };

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

#[allow(dead_code)]
fn create_and_destroy_kapsule_cluster(
    region: Region,
    secrets: FuncTestsSecrets,
    test_infra_pause: bool,
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
        let cloudflare = dns_provider_cloudflare(&context);

        let cluster_id = format!("qovery-test-{}", generate_cluster_id(region.as_str()));

        let kubernetes = Kapsule::new(
            context,
            cluster_id.clone(),
            cluster_id,
            SCW_KUBERNETES_VERSION.to_string(),
            region,
            &scw_cluster,
            &cloudflare,
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

        if test_infra_pause {
            // Pause
            if let Err(err) = tx.pause_kubernetes(&kubernetes) {
                panic!("{:?}", err)
            }
            match tx.commit() {
                TransactionResult::Ok => assert!(true),
                TransactionResult::Rollback(_) => assert!(false),
                TransactionResult::UnrecoverableError(_, _) => assert!(false),
            };

            // Resume
            if let Err(err) = tx.create_kubernetes(&kubernetes) {
                panic!("{:?}", err)
            }
            let _ = match tx.commit() {
                TransactionResult::Ok => assert!(true),
                TransactionResult::Rollback(_) => assert!(false),
                TransactionResult::UnrecoverableError(_, _) => assert!(false),
            };
        }

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

#[cfg(feature = "test-scw-infra")]
#[test]
fn create_and_destroy_kapsule_cluster_par() {
    let region = Region::Paris;
    let secrets = FuncTestsSecrets::new();
    create_and_destroy_kapsule_cluster(
        region,
        secrets,
        false,
        &format!(
            "create_and_destroy_kapsule_cluster_in_{}",
            region.as_str().replace("-", "_")
        ),
    );
}

// only enable this test manually when we want to perform and validate upgrade process
//#[test]
#[allow(dead_code)]
fn create_upgrade_and_destroy_kapsule_cluster_in_fr_par() {
    let region = Region::Paris;
    let secrets = FuncTestsSecrets::new();
    create_upgrade_and_destroy_kapsule_cluster(
        region,
        secrets,
        "1.18",
        "1.19",
        &format!(
            "create_upgrade_and_destroy_kapsule_cluster_in_{}",
            region.as_str().replace("-", "_")
        ),
    );
}

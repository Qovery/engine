extern crate test_utilities;

use self::test_utilities::cloudflare::dns_provider_cloudflare;
use self::test_utilities::utilities::{context, engine_run_test, generate_cluster_id, init, FuncTestsSecrets};
use ::function_name::named;
use tracing::{span, Level};

use qovery_engine::cloud_provider::scaleway::application::Zone;
use qovery_engine::cloud_provider::scaleway::kubernetes::Kapsule;
use qovery_engine::transaction::TransactionResult;

use test_utilities::scaleway::SCW_KUBERNETES_VERSION;

#[allow(dead_code)]
fn create_upgrade_and_destroy_kapsule_cluster(
    zone: Zone,
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

        let cluster_id = generate_cluster_id(zone.as_str());

        let kubernetes = Kapsule::new(
            context,
            cluster_id.clone(),
            uuid::Uuid::new_v4(),
            cluster_id,
            boot_version.to_string(),
            zone,
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
fn create_and_destroy_kapsule_cluster(zone: Zone, secrets: FuncTestsSecrets, test_infra_pause: bool, test_name: &str) {
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

        let cluster_id = generate_cluster_id(zone.as_str());

        let kubernetes = Kapsule::new(
            context,
            cluster_id.clone(),
            uuid::Uuid::new_v4(),
            cluster_id,
            SCW_KUBERNETES_VERSION.to_string(),
            zone,
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
#[named]
#[test]
fn create_and_destroy_kapsule_cluster_par_1() {
    let zone = Zone::Paris1;
    let secrets = FuncTestsSecrets::new();
    create_and_destroy_kapsule_cluster(zone, secrets, false, function_name!());
}

#[cfg(feature = "test-scw-infra")]
#[named]
#[test]
#[ignore]
#[allow(dead_code)]
fn create_and_destroy_kapsule_cluster_par_2() {
    let zone = Zone::Paris2;
    let secrets = FuncTestsSecrets::new();
    create_and_destroy_kapsule_cluster(zone, secrets, false, function_name!());
}

#[cfg(feature = "test-scw-infra")]
#[named]
#[test]
#[ignore]
#[allow(dead_code)]
fn create_and_destroy_kapsule_cluster_ams_1() {
    let zone = Zone::Amsterdam1;
    let secrets = FuncTestsSecrets::new();
    create_and_destroy_kapsule_cluster(zone, secrets, false, function_name!());
}

#[cfg(feature = "test-scw-infra")]
#[named]
#[test]
fn create_and_destroy_kapsule_cluster_war_1() {
    let zone = Zone::Warsaw1;
    let secrets = FuncTestsSecrets::new();
    create_and_destroy_kapsule_cluster(zone, secrets, false, function_name!());
}

// only enable this test manually when we want to perform and validate upgrade process
#[test]
#[ignore]
#[allow(dead_code)]
#[allow(unused_attributes)]
#[named]
fn create_upgrade_and_destroy_kapsule_cluster_in_par_1() {
    let zone = Zone::Paris1;
    let secrets = FuncTestsSecrets::new();
    create_upgrade_and_destroy_kapsule_cluster(zone, secrets, "1.18", "1.19", function_name!());
}

// only enable this test manually when we want to perform and validate upgrade process
#[test]
#[ignore]
#[allow(unused_attributes)]
#[allow(dead_code)]
#[named]
fn create_upgrade_and_destroy_kapsule_cluster_in_par_2() {
    let zone = Zone::Paris2;
    let secrets = FuncTestsSecrets::new();
    create_upgrade_and_destroy_kapsule_cluster(zone, secrets, "1.18", "1.19", function_name!());
}

// only enable this test manually when we want to perform and validate upgrade process
#[test]
#[ignore]
#[allow(unused_attributes)]
#[allow(dead_code)]
#[named]
fn create_upgrade_and_destroy_kapsule_cluster_in_ams_1() {
    let zone = Zone::Amsterdam1;
    let secrets = FuncTestsSecrets::new();
    create_upgrade_and_destroy_kapsule_cluster(zone, secrets, "1.18", "1.19", function_name!());
}

// only enable this test manually when we want to perform and validate upgrade process
#[test]
#[ignore]
#[allow(unused_attributes)]
#[allow(dead_code)]
#[named]
fn create_upgrade_and_destroy_kapsule_cluster_in_war_1() {
    let zone = Zone::Warsaw1;
    let secrets = FuncTestsSecrets::new();
    create_upgrade_and_destroy_kapsule_cluster(zone, secrets, "1.18", "1.19", function_name!());
}

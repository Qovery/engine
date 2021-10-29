extern crate test_utilities;

use self::test_utilities::cloudflare::dns_provider_cloudflare;
use self::test_utilities::utilities::{context, engine_run_test, generate_cluster_id, init, FuncTestsSecrets};
use ::function_name::named;
use tracing::{span, Level};

use qovery_engine::cloud_provider::digitalocean::kubernetes::DOKS;
use qovery_engine::transaction::TransactionResult;

use qovery_engine::cloud_provider::digitalocean::application::Region;
use test_utilities::digitalocean::DO_KUBERNETES_VERSION;

#[cfg(test)]
fn create_upgrade_and_destroy_doks_cluster(
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
        let engine = test_utilities::digitalocean::docker_cr_do_engine(&context);
        let session = engine.session().unwrap();
        let mut tx = session.transaction();

        let do_cluster = test_utilities::digitalocean::cloud_provider_digitalocean(&context);
        let nodes = test_utilities::digitalocean::do_kubernetes_nodes();
        let cloudflare = dns_provider_cloudflare(&context);

        let cluster_id = generate_cluster_id(region.as_str());

        let kubernetes = DOKS::new(
            context,
            cluster_id.clone(),
            uuid::Uuid::new_v4(),
            cluster_id.clone(),
            boot_version.to_string(),
            region,
            &do_cluster,
            &cloudflare,
            nodes,
            test_utilities::digitalocean::do_kubernetes_cluster_options(secrets, cluster_id),
        )
        .unwrap();

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

#[cfg(test)]
fn create_and_destroy_doks_cluster(region: Region, secrets: FuncTestsSecrets, test_infra_pause: bool, test_name: &str) {
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let context = context();
        let engine = test_utilities::digitalocean::docker_cr_do_engine(&context);
        let session = engine.session().unwrap();
        let mut tx = session.transaction();

        let do_cluster = test_utilities::digitalocean::cloud_provider_digitalocean(&context);
        let nodes = test_utilities::digitalocean::do_kubernetes_nodes();
        let cloudflare = dns_provider_cloudflare(&context);

        let cluster_id = generate_cluster_id(region.as_str());

        let kubernetes = DOKS::new(
            context,
            cluster_id.clone(),
            uuid::Uuid::new_v4(),
            cluster_id.clone(),
            DO_KUBERNETES_VERSION.to_string(),
            region,
            &do_cluster,
            &cloudflare,
            nodes,
            test_utilities::digitalocean::do_kubernetes_cluster_options(secrets, cluster_id),
        )
        .unwrap();

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

#[cfg(feature = "test-do-infra")]
#[ignore]
#[named]
#[test]
fn create_and_destroy_doks_cluster_ams_3() {
    let region = Region::Amsterdam3;
    let secrets = FuncTestsSecrets::new();
    create_and_destroy_doks_cluster(region, secrets, false, function_name!());
}

#[test]
#[ignore]
#[named]
fn create_upgrade_and_destroy_doks_cluster_in_nyc_3() {
    let region = Region::NewYorkCity3;
    let secrets = FuncTestsSecrets::new();
    create_upgrade_and_destroy_doks_cluster(region, secrets, "1.19", "1.20", function_name!());
}

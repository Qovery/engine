pub use ::function_name::named;
pub use tracing::{span, Level};

pub use qovery_engine::cloud_provider::scaleway::application::Zone;
pub use qovery_engine::cloud_provider::scaleway::kubernetes::Kapsule;
pub use qovery_engine::transaction::TransactionResult;

pub use crate::helpers::cloudflare::dns_provider_cloudflare;
pub use crate::helpers::scaleway::{
    cloud_provider_scaleway, docker_scw_cr_engine, scw_kubernetes_cluster_options, scw_kubernetes_nodes,
    SCW_KUBERNETES_VERSION,
};
pub use crate::helpers::utilities::{context, engine_run_test, generate_cluster_id, init, FuncTestsSecrets};

#[cfg(test)]
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
        let engine = docker_scw_cr_engine(&context);
        let session = engine.session().unwrap();
        let mut tx = session.transaction();

        let scw_cluster = cloud_provider_scaleway(&context);
        let nodes = scw_kubernetes_nodes();
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
            scw_kubernetes_cluster_options(secrets),
        )
        .unwrap();

        // Deploy
        if let Err(err) = tx.create_kubernetes(&kubernetes) {
            panic!("{:?}", err)
        }
        let _ = match tx.commit() {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        // Upgrade
        // TODO(benjaminch): To be added
        //let kubernetes = ...
        // if let Err(err) = tx.create_kubernetes(&kubernetes) {
        //     panic!("{:?}", err)
        // }
        // let _ = match tx.commit() {
        //     TransactionResult::Ok => {},
        //     TransactionResult::Rollback(_) => panic!(),
        //     TransactionResult::UnrecoverableError(_, _) => panic!(),
        // };

        // Destroy
        if let Err(err) = tx.delete_kubernetes(&kubernetes) {
            panic!("{:?}", err)
        }
        match tx.commit() {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        test_name.to_string()
    });
}

#[cfg(test)]
fn create_and_destroy_kapsule_cluster(zone: Zone, secrets: FuncTestsSecrets, test_infra_pause: bool, test_name: &str) {
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let context = context();
        let engine = docker_scw_cr_engine(&context);
        let session = engine.session().unwrap();
        let mut tx = session.transaction();

        let scw_cluster = cloud_provider_scaleway(&context);
        let nodes = scw_kubernetes_nodes();
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
            scw_kubernetes_cluster_options(secrets),
        )
        .unwrap();

        // Deploy
        if let Err(err) = tx.create_kubernetes(&kubernetes) {
            panic!("{:?}", err)
        }
        let _ = match tx.commit() {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        if test_infra_pause {
            // Pause
            if let Err(err) = tx.pause_kubernetes(&kubernetes) {
                panic!("{:?}", err)
            }
            match tx.commit() {
                TransactionResult::Ok => {}
                TransactionResult::Rollback(_) => panic!(),
                TransactionResult::UnrecoverableError(_, _) => panic!(),
            };

            // Resume
            if let Err(err) = tx.create_kubernetes(&kubernetes) {
                panic!("{:?}", err)
            }
            let _ = match tx.commit() {
                TransactionResult::Ok => {}
                TransactionResult::Rollback(_) => panic!(),
                TransactionResult::UnrecoverableError(_, _) => panic!(),
            };
        }

        // Destroy
        if let Err(err) = tx.delete_kubernetes(&kubernetes) {
            panic!("{:?}", err)
        }
        match tx.commit() {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
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
fn create_and_destroy_kapsule_cluster_par_2() {
    let zone = Zone::Paris2;
    let secrets = FuncTestsSecrets::new();
    create_and_destroy_kapsule_cluster(zone, secrets, false, function_name!());
}

#[cfg(feature = "test-scw-infra")]
#[named]
#[test]
#[ignore]
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
#[named]
fn create_upgrade_and_destroy_kapsule_cluster_in_par_1() {
    let zone = Zone::Paris1;
    let secrets = FuncTestsSecrets::new();
    create_upgrade_and_destroy_kapsule_cluster(zone, secrets, "1.18", "1.19", function_name!());
}

// only enable this test manually when we want to perform and validate upgrade process
#[test]
#[ignore]
#[named]
fn create_upgrade_and_destroy_kapsule_cluster_in_par_2() {
    let zone = Zone::Paris2;
    let secrets = FuncTestsSecrets::new();
    create_upgrade_and_destroy_kapsule_cluster(zone, secrets, "1.18", "1.19", function_name!());
}

// only enable this test manually when we want to perform and validate upgrade process
#[test]
#[ignore]
#[named]
fn create_upgrade_and_destroy_kapsule_cluster_in_ams_1() {
    let zone = Zone::Amsterdam1;
    let secrets = FuncTestsSecrets::new();
    create_upgrade_and_destroy_kapsule_cluster(zone, secrets, "1.18", "1.19", function_name!());
}

// only enable this test manually when we want to perform and validate upgrade process
#[test]
#[ignore]
#[named]
fn create_upgrade_and_destroy_kapsule_cluster_in_war_1() {
    let zone = Zone::Warsaw1;
    let secrets = FuncTestsSecrets::new();
    create_upgrade_and_destroy_kapsule_cluster(zone, secrets, "1.18", "1.19", function_name!());
}

use ::function_name::named;
use qovery_engine::cloud_provider::scaleway::application::Zone;
use qovery_engine::cloud_provider::scaleway::kubernetes::Kapsule;
use qovery_engine::cloud_provider::scaleway::Scaleway;
use qovery_engine::cloud_provider::CloudProvider;
use qovery_engine::models::{Context, EnvironmentAction};
use qovery_engine::transaction::{DeploymentOption, TransactionResult};
use test_utilities::cloudflare::{dns_provider_cloudflare, CloudflareDomain};
use test_utilities::common::Cluster;
use test_utilities::utilities::{context, engine_run_test, generate_cluster_id, generate_id, init, FuncTestsSecrets};
use tracing::{span, Level};

#[allow(dead_code)]
fn create_upgrade_and_destroy_kapsule_cluster_and_env(
    context: Context,
    cluster_id: &str,
    cluster_domain: &str,
    zone: Zone,
    secrets: FuncTestsSecrets,
    boot_version: &str,
    _upgrade_to_version: &str,
    environment_action: EnvironmentAction,
    test_name: &str,
) {
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let engine = Scaleway::docker_cr_engine(&context);
        let scw_cluster: Box<dyn CloudProvider> = Scaleway::cloud_provider(&context);
        let nodes = Scaleway::kubernetes_nodes();
        let cloudflare = dns_provider_cloudflare(&context, CloudflareDomain::Custom(cluster_domain.to_string()));

        let kapsule = Kapsule::new(
            context,
            cluster_id.to_string(),
            uuid::Uuid::new_v4(),
            cluster_id.to_string(),
            boot_version.to_string(),
            zone,
            scw_cluster.as_ref(),
            &cloudflare,
            nodes,
            Scaleway::kubernetes_cluster_options(secrets, Some(cluster_id.to_string())),
        )
        .unwrap();

        // Deploy infrastructure
        let mut tx = engine.session().unwrap().transaction();
        if let Err(err) = tx.create_kubernetes(&kapsule) {
            panic!("{:?}", err)
        }
        let _ = match tx.commit() {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        // Deploy env
        let mut tx = engine.session().unwrap().transaction();
        let _ = tx.deploy_environment_with_options(
            &kapsule,
            &environment_action,
            DeploymentOption {
                force_build: false,
                force_push: false,
            },
        );

        match tx.commit() {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        }

        // Upgrade infrastructure
        // TODO(benjaminch): To be added
        // let kubernetes = ...
        // if let Err(err) = tx.create_kubernetes(&kubernetes) {
        //     panic!("{:?}", err)
        // }
        // let _ = match tx.commit() {
        //     TransactionResult::Ok => assert!(true),
        //     TransactionResult::Rollback(_) => assert!(false),
        //     TransactionResult::UnrecoverableError(_, _) => assert!(false),
        // };

        // Destroy env
        let mut tx = engine.session().unwrap().transaction();
        let _ = tx.delete_environment(&kapsule, &environment_action);
        match tx.commit() {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        // Destroy infrastructure
        let mut tx = engine.session().unwrap().transaction();
        if let Err(err) = tx.delete_kubernetes(&kapsule) {
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

#[cfg(feature = "test-scw-whole-enchilada")]
#[named]
#[test]
fn create_upgrade_and_destroy_kapsule_cluster_with_env_in_par_2() {
    let context = context();
    let zone = Zone::Paris2;
    let secrets = FuncTestsSecrets::new();
    let organization_id = generate_id();
    let cluster_id = generate_cluster_id(zone.as_str());
    let cluster_domain = format!(
        "{}.{}",
        cluster_id.as_str(),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str()
    );

    let environment = test_utilities::common::working_minimal_environment(
        &context,
        organization_id.as_str(),
        cluster_domain.as_str(),
    );
    let env_action = EnvironmentAction::Environment(environment.clone());

    create_upgrade_and_destroy_kapsule_cluster_and_env(
        context,
        cluster_id.as_str(),
        cluster_domain.as_str(),
        zone,
        secrets,
        "1.18",
        "1.19",
        env_action,
        function_name!(),
    );
}

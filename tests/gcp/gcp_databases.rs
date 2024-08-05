use crate::helpers;
use crate::helpers::common::{ClusterDomain, Infrastructure};
use crate::helpers::database::{database_test_environment, test_db, StorageSize};
use crate::helpers::gcp::{
    clean_environments, gcp_default_infra_config, GCP_MANAGED_DATABASE_DISK_TYPE, GCP_MANAGED_DATABASE_INSTANCE_TYPE,
    GCP_SELF_HOSTED_DATABASE_DISK_TYPE,
};
use crate::helpers::utilities::{
    context_for_resource, engine_run_test, generate_password, get_pods, get_svc_name, init, is_pod_restarted_env,
    logger, metrics_registry, FuncTestsSecrets,
};
use base64::engine::general_purpose;
use base64::Engine;
use function_name::named;
use qovery_engine::cloud_provider::gcp::locations::GcpRegion;
use qovery_engine::cloud_provider::kubernetes::Kind as KubernetesKind;
use qovery_engine::cloud_provider::{Kind as ProviderKind, Kind};
use qovery_engine::io_models::application::{Port, Protocol};
use qovery_engine::io_models::context::CloneForTest;
use qovery_engine::io_models::database::DatabaseMode::{CONTAINER, MANAGED};
use qovery_engine::io_models::database::{Database, DatabaseKind, DatabaseMode};
use qovery_engine::io_models::probe::{Probe, ProbeType};
use qovery_engine::io_models::variable_utils::VariableInfo;
use qovery_engine::io_models::{Action, QoveryIdentifier};
use qovery_engine::transaction::TransactionResult;
use qovery_engine::utilities::to_short_id;
use std::str::FromStr;
use tracing::{span, warn, Level};
use uuid::Uuid;

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn deploy_an_environment_with_3_databases_and_3_apps() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let secrets = FuncTestsSecrets::new();
        let logger = logger();
        let metrics_registry = metrics_registry();
        let context = context_for_resource(
            secrets
                .GCP_TEST_ORGANIZATION_LONG_ID
                .expect("GCP_TEST_ORGANIZATION_LONG_ID is not set"),
            secrets
                .GCP_TEST_CLUSTER_LONG_ID
                .expect("GCP_TEST_CLUSTER_LONG_ID is not set"),
        );
        let infra_ctx = gcp_default_infra_config(&context, logger.clone(), metrics_registry.clone());
        let context_for_deletion = context.clone_not_same_execution_id();
        let infra_ctx_for_deletion =
            gcp_default_infra_config(&context_for_deletion, logger.clone(), metrics_registry.clone());
        let environment = helpers::database::environment_3_apps_3_databases(
            &context,
            None,
            &GCP_SELF_HOSTED_DATABASE_DISK_TYPE.to_k8s_storage_class(),
            Kind::Gcp,
        );

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;
        let ea = environment.clone();
        let ea_delete = environment_delete.clone();

        let ret = environment.deploy_environment(&ea, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = environment_delete.delete_environment(&ea_delete, &infra_ctx_for_deletion);
        assert!(matches!(ret, TransactionResult::Ok));

        test_name.to_string()
    })
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn deploy_an_environment_with_db_and_pause_it() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let secrets = FuncTestsSecrets::new();
        let logger = logger();
        let metrics_registry = metrics_registry();
        let cluster_id = secrets
            .GCP_TEST_CLUSTER_LONG_ID
            .expect("GCP_TEST_CLUSTER_LONG_ID is not set");
        let context = context_for_resource(
            secrets
                .GCP_TEST_ORGANIZATION_LONG_ID
                .expect("GCP_TEST_ORGANIZATION_LONG_ID is not set"),
            cluster_id,
        );
        let infra_ctx = gcp_default_infra_config(&context, logger.clone(), metrics_registry.clone());
        let context_for_deletion = context.clone_not_same_execution_id();
        let infra_ctx_for_deletion =
            gcp_default_infra_config(&context_for_deletion, logger.clone(), metrics_registry.clone());
        let environment = helpers::environment::environment_2_app_2_routers_1_psql(
            &context,
            secrets
                .clone()
                .DEFAULT_TEST_DOMAIN
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
            None,
            &GCP_SELF_HOSTED_DATABASE_DISK_TYPE.to_k8s_storage_class(),
            Kind::Gcp,
        );

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;
        let ea = environment.clone();
        let ea_delete = environment_delete.clone();

        let ret = environment.deploy_environment(&ea, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = environment.pause_environment(&ea, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        // Check that we have actually 0 pods running for this db
        let ret = get_pods(&infra_ctx, Kind::Gcp, &environment, &environment.databases[0].long_id, secrets);
        assert!(ret.is_ok());
        assert!(ret.unwrap().items.is_empty());

        let ret = environment_delete.delete_environment(&ea_delete, &infra_ctx_for_deletion);
        assert!(matches!(ret, TransactionResult::Ok));

        test_name.to_string()
    })
}

// Ensure a full environment can run correctly
#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn postgresql_deploy_a_working_development_environment_with_all_options() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let cluster_id = secrets.GCP_TEST_CLUSTER_LONG_ID.expect("GCP_TEST_CLUSTER_LONG_ID");
        let context = context_for_resource(
            secrets
                .GCP_TEST_ORGANIZATION_LONG_ID
                .expect("GCP_TEST_ORGANIZATION_LONG_ID"),
            cluster_id,
        );

        let infra_ctx = gcp_default_infra_config(&context, logger.clone(), metrics_registry.clone());
        let context_for_deletion = context.clone_not_same_execution_id();
        let infra_ctx_for_deletion =
            gcp_default_infra_config(&context_for_deletion, logger.clone(), metrics_registry.clone());
        let test_domain = secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets");

        let environment = helpers::environment::environment_2_app_2_routers_1_psql(
            &context,
            test_domain.as_str(),
            None,
            &GCP_SELF_HOSTED_DATABASE_DISK_TYPE.to_k8s_storage_class(),
            Kind::Gcp,
        );
        let mut environment_delete = helpers::environment::environment_2_app_2_routers_1_psql(
            &context_for_deletion,
            test_domain.as_str(),
            None,
            &GCP_SELF_HOSTED_DATABASE_DISK_TYPE.to_k8s_storage_class(),
            Kind::Gcp,
        );

        environment_delete.action = Action::Delete;

        let env_action = environment.clone();
        let env_action_for_deletion = environment_delete.clone();

        let result = environment.deploy_environment(&env_action, &infra_ctx);
        assert!(matches!(result, TransactionResult::Ok));

        let result = environment_delete.delete_environment(&env_action_for_deletion, &infra_ctx_for_deletion);
        assert!(matches!(result, TransactionResult::Ok));

        // delete images created during test from registries
        if let Err(e) = clean_environments(
            &context,
            vec![environment, environment_delete],
            secrets.clone(),
            GcpRegion::from_str(
                secrets
                    .GCP_DEFAULT_REGION
                    .expect("GCP_DEFAULT_REGION is not set")
                    .as_str(),
            )
            .expect("Unknown GCP region"),
        ) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

// Ensure redeploy works as expected
#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn postgresql_deploy_a_working_environment_and_redeploy() {
    use chrono::Utc;

    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let cluster_id = secrets.GCP_TEST_CLUSTER_LONG_ID.expect("GCP_TEST_CLUSTER_LONG_ID");
        let region = GcpRegion::from_str(
            secrets
                .GCP_DEFAULT_REGION
                .as_ref()
                .expect("GCP_DEFAULT_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown GCP region");
        let context = context_for_resource(
            secrets
                .GCP_TEST_ORGANIZATION_LONG_ID
                .expect("GCP_TEST_ORGANIZATION_LONG_ID"),
            cluster_id,
        );

        let infra_ctx = gcp_default_infra_config(&context, logger.clone(), metrics_registry.clone());
        let context_for_redeploy = context.clone_not_same_execution_id();
        let infra_ctx_for_redeploy =
            gcp_default_infra_config(&context_for_redeploy, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete =
            gcp_default_infra_config(&context_for_delete, logger.clone(), metrics_registry.clone());

        let mut environment = helpers::environment::working_minimal_environment(&context);

        let app_name = format!("pg-app-{}", QoveryIdentifier::new_random().short());
        let database_mode = CONTAINER;
        let database_host = get_svc_name(DatabaseKind::Postgresql, Kind::Gcp).to_string();
        let database_port = 5432;
        let database_db_name = "pg".to_string();
        let database_username = "superuser".to_string();
        let database_password = generate_password(CONTAINER);

        environment.databases = vec![Database {
            kind: DatabaseKind::Postgresql,
            action: Action::Create,
            long_id: Uuid::new_v4(),
            name: database_db_name.clone(),
            kube_name: database_db_name.clone(),
            created_at: Utc::now(),
            version: "11.22.0".to_string(),
            fqdn_id: database_host.clone(),
            fqdn: database_host.clone(),
            port: database_port,
            username: database_username.clone(),
            password: database_password.clone(),
            cpu_request_in_milli: 500,
            cpu_limit_in_milli: 500,
            ram_request_in_mib: 512,
            ram_limit_in_mib: 512,
            disk_size_in_gib: 10,
            mode: database_mode.clone(),
            database_instance_type: if database_mode == MANAGED {
                Some(GCP_MANAGED_DATABASE_INSTANCE_TYPE.to_string())
            } else {
                None
            },
            database_disk_type: if database_mode == MANAGED {
                GCP_MANAGED_DATABASE_DISK_TYPE.to_string()
            } else {
                GCP_SELF_HOSTED_DATABASE_DISK_TYPE.to_k8s_storage_class()
            }
            .to_string(),
            encrypt_disk: false,
            activate_high_availability: false,
            activate_backups: false,
            publicly_accessible: false,
            annotations_group_ids: btreeset! {},
            labels_group_ids: btreeset! {},
        }];
        environment.applications = environment
            .applications
            .into_iter()
            .map(|mut app| {
                app.branch.clone_from(&app_name);
                app.commit_id = "5990752647af11ef21c3d46a51abbde3da1ab351".to_string();
                app.ports = vec![Port {
                    long_id: Default::default(),
                    port: 1234,
                    name: "p1234".to_string(),
                    is_default: true,
                    publicly_accessible: true,
                    protocol: Protocol::HTTP,
                    service_name: None,
                    namespace: None,
                }];
                app.readiness_probe = Some(Probe {
                    r#type: ProbeType::Tcp { host: None },
                    port: 1234,
                    failure_threshold: 9,
                    success_threshold: 1,
                    initial_delay_seconds: 15,
                    period_seconds: 10,
                    timeout_seconds: 10,
                });
                app.liveness_probe = None;
                app.environment_vars_with_infos = btreemap! {
                     "PG_DBNAME".to_string() => VariableInfo{ value: general_purpose::STANDARD.encode(database_db_name.clone()), is_secret:false},
                     "PG_HOST".to_string() => VariableInfo{ value:general_purpose::STANDARD.encode(database_host.clone()), is_secret:false},
                     "PG_PORT".to_string() => VariableInfo{ value:general_purpose::STANDARD.encode(database_port.to_string()), is_secret:false},
                     "PG_USERNAME".to_string() => VariableInfo{ value:general_purpose::STANDARD.encode(database_username.clone()), is_secret:false},
                     "PG_PASSWORD".to_string() => VariableInfo{ value:general_purpose::STANDARD.encode(database_password.clone()), is_secret:false},
                };
                app
            })
            .collect::<Vec<qovery_engine::io_models::application::Application>>();

        let environment_to_redeploy = environment.clone();
        let environment_check = environment.clone();
        let env_action_redeploy = environment_to_redeploy.clone();

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;
        let env_action = environment.clone();
        let env_action_delete = environment_delete.clone();

        let result = environment.deploy_environment(&env_action, &infra_ctx);
        assert!(matches!(result, TransactionResult::Ok));

        let result = environment_to_redeploy.deploy_environment(&env_action_redeploy, &infra_ctx_for_redeploy);
        assert!(matches!(result, TransactionResult::Ok));

        // TO CHECK: DATABASE SHOULDN'T BE RESTARTED AFTER A REDEPLOY
        let (ret, _) = is_pod_restarted_env(
            &infra_ctx,
            ProviderKind::Gcp,
            &environment_check,
            &environment_check.databases[0].long_id,
            secrets.clone(),
        );
        assert!(ret);

        let result = environment_delete.delete_environment(&env_action_delete, &infra_ctx_for_delete);
        assert!(matches!(result, TransactionResult::Ok | TransactionResult::Error(_)));

        // delete images created during test from registries
        if let Err(e) = clean_environments(&context, vec![environment], secrets, region) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn test_oversized_volume() {
    let secrets = FuncTestsSecrets::new();
    let cluster_id = secrets.GCP_TEST_CLUSTER_LONG_ID.expect("GCP_TEST_CLUSTER_LONG_ID");
    let region = secrets
        .GCP_DEFAULT_REGION
        .as_ref()
        .expect("GCP_DEFAULT_REGION is not set")
        .to_string();
    let context = context_for_resource(
        secrets
            .GCP_TEST_ORGANIZATION_LONG_ID
            .expect("GCP_TEST_ORGANIZATION_LONG_ID"),
        cluster_id,
    );
    let environment = database_test_environment(&context);

    engine_run_test(|| {
        test_db(
            context,
            logger(),
            metrics_registry(),
            environment,
            secrets,
            "13",
            function_name!(),
            DatabaseKind::Postgresql,
            KubernetesKind::Gke,
            CONTAINER,
            region,
            false,
            ClusterDomain::Default {
                cluster_id: to_short_id(&cluster_id),
            },
            None,
            StorageSize::OverSize,
        )
    })
}

/**
 **
 ** PostgreSQL tests
 **
 **/
#[allow(dead_code)]
fn test_postgresql_configuration(version: &str, test_name: &str, database_mode: DatabaseMode, is_public: bool) {
    let secrets = FuncTestsSecrets::new();
    let cluster_id = secrets.GCP_TEST_CLUSTER_LONG_ID.expect("GCP_TEST_CLUSTER_LONG_ID");
    let region = secrets
        .GCP_DEFAULT_REGION
        .as_ref()
        .expect("GCP_DEFAULT_REGION is not set")
        .to_string();
    let context = context_for_resource(
        secrets
            .GCP_TEST_ORGANIZATION_LONG_ID
            .expect("GCP_TEST_ORGANIZATION_LONG_ID"),
        cluster_id,
    );
    let environment = database_test_environment(&context);

    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        test_db(
            context,
            logger(),
            metrics_registry(),
            environment,
            secrets,
            version,
            test_name,
            DatabaseKind::Postgresql,
            KubernetesKind::Gke,
            database_mode,
            region,
            is_public,
            ClusterDomain::Default {
                cluster_id: to_short_id(&cluster_id),
            },
            None,
            StorageSize::NormalSize,
        )
    })
}

// Postgres self hosted environment
#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
#[ignore = "No database deployed in this version"]
fn private_postgresql_v10_deploy_a_working_dev_environment() {
    test_postgresql_configuration("10", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
#[ignore = "No database deployed in this version"]
fn public_postgresql_v10_deploy_a_working_dev_environment() {
    test_postgresql_configuration("10", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn private_postgresql_v11_deploy_a_working_dev_environment() {
    test_postgresql_configuration("11", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
#[ignore]
fn public_postgresql_v11_deploy_a_working_dev_environment() {
    test_postgresql_configuration("11", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn private_postgresql_v12_deploy_a_working_dev_environment() {
    test_postgresql_configuration("12", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
#[ignore]
fn public_postgresql_v12_deploy_a_working_dev_environment() {
    test_postgresql_configuration("12", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn private_postgresql_v13_deploy_a_working_dev_environment() {
    test_postgresql_configuration("13", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
#[ignore]
fn public_postgresql_v13_deploy_a_working_dev_environment() {
    test_postgresql_configuration("13", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn private_postgresql_v14_deploy_a_working_dev_environment() {
    test_postgresql_configuration("14", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn public_postgresql_v14_deploy_a_working_dev_environment() {
    test_postgresql_configuration("14", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn private_postgresql_v15_deploy_a_working_dev_environment() {
    test_postgresql_configuration("15", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn public_postgresql_v15_deploy_a_working_dev_environment() {
    test_postgresql_configuration("15", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn private_postgresql_v16_deploy_a_working_dev_environment() {
    test_postgresql_configuration("16", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn public_postgresql_v16_deploy_a_working_dev_environment() {
    test_postgresql_configuration("16", function_name!(), CONTAINER, true);
}

/**
 **
 ** MongoDB tests
 **
 **/
#[allow(dead_code)]
fn test_mongodb_configuration(version: &str, test_name: &str, database_mode: DatabaseMode, is_public: bool) {
    let secrets = FuncTestsSecrets::new();
    let cluster_id = secrets.GCP_TEST_CLUSTER_LONG_ID.expect("GCP_TEST_CLUSTER_LONG_ID");
    let region = secrets
        .GCP_DEFAULT_REGION
        .as_ref()
        .expect("GCP_DEFAULT_REGION is not set")
        .to_string();
    let context = context_for_resource(
        secrets
            .GCP_TEST_ORGANIZATION_LONG_ID
            .expect("GCP_TEST_ORGANIZATION_LONG_ID"),
        cluster_id,
    );
    let environment = database_test_environment(&context);

    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        test_db(
            context,
            logger(),
            metrics_registry(),
            environment,
            secrets,
            version,
            test_name,
            DatabaseKind::Mongodb,
            KubernetesKind::Gke,
            database_mode,
            region,
            is_public,
            ClusterDomain::Default {
                cluster_id: to_short_id(&cluster_id),
            },
            None,
            StorageSize::NormalSize,
        )
    })
}

// development environment
#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn private_mongodb_v4_2_deploy_a_working_dev_environment() {
    test_mongodb_configuration("4.2", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
#[ignore]
fn public_mongodb_v4_2_deploy_a_working_dev_environment() {
    test_mongodb_configuration("4.2", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn private_mongodb_v4_4_deploy_a_working_dev_environment() {
    test_mongodb_configuration("4.4", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn public_mongodb_v4_4_deploy_a_working_dev_environment() {
    test_mongodb_configuration("4.4", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn private_mongodb_v5_0_deploy_a_working_dev_environment() {
    test_mongodb_configuration("5.0", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn public_mongodb_v5_0_deploy_a_working_dev_environment() {
    test_mongodb_configuration("5.0", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn private_mongodb_v6_0_deploy_a_working_dev_environment() {
    test_mongodb_configuration("6.0", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn public_mongodb_v6_0_deploy_a_working_dev_environment() {
    test_mongodb_configuration("6.0", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn private_mongodb_v7_0_deploy_a_working_dev_environment() {
    test_mongodb_configuration("7.0", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn public_mongodb_v7_0_deploy_a_working_dev_environment() {
    test_mongodb_configuration("7.0", function_name!(), CONTAINER, true);
}

/**
 **
 ** MySQL tests
 **
 **/
#[allow(dead_code)]
fn test_mysql_configuration(version: &str, test_name: &str, database_mode: DatabaseMode, is_public: bool) {
    let secrets = FuncTestsSecrets::new();
    let cluster_id = secrets.GCP_TEST_CLUSTER_LONG_ID.expect("GCP_TEST_CLUSTER_LONG_ID");
    let region = secrets
        .GCP_DEFAULT_REGION
        .as_ref()
        .expect("GCP_DEFAULT_REGION is not set")
        .to_string();
    let context = context_for_resource(
        secrets
            .GCP_TEST_ORGANIZATION_LONG_ID
            .expect("GCP_TEST_ORGANIZATION_LONG_ID"),
        cluster_id,
    );
    let environment = database_test_environment(&context);

    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        test_db(
            context,
            logger(),
            metrics_registry(),
            environment,
            secrets,
            version,
            test_name,
            DatabaseKind::Mysql,
            KubernetesKind::Gke,
            database_mode,
            region,
            is_public,
            ClusterDomain::Default {
                cluster_id: to_short_id(&cluster_id),
            },
            None,
            StorageSize::NormalSize,
        )
    })
}

// MySQL self-hosted environment
#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
#[ignore = "Only 1 deployed in this version"]
fn private_mysql_v5_7_deploy_a_working_dev_environment() {
    test_mysql_configuration("5.7", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
#[ignore = "Only 1 deployed in this version"]
fn public_mysql_v5_7_deploy_a_working_dev_environment() {
    test_mysql_configuration("5.7", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn private_mysql_v8_deploy_a_working_dev_environment() {
    test_mysql_configuration("8.0", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn public_mysql_v8_deploy_a_working_dev_environment() {
    test_mysql_configuration("8.0", function_name!(), CONTAINER, true);
}

/**
 **
 ** Redis tests
 **
 **/
#[allow(dead_code)]
fn test_redis_configuration(version: &str, test_name: &str, database_mode: DatabaseMode, is_public: bool) {
    let secrets = FuncTestsSecrets::new();
    let cluster_id = secrets.GCP_TEST_CLUSTER_LONG_ID.expect("GCP_TEST_CLUSTER_LONG_ID");
    let region = secrets
        .GCP_DEFAULT_REGION
        .as_ref()
        .expect("GCP_DEFAULT_REGION is not set")
        .to_string();
    let context = context_for_resource(
        secrets
            .GCP_TEST_ORGANIZATION_LONG_ID
            .expect("GCP_TEST_ORGANIZATION_LONG_ID"),
        cluster_id,
    );
    let environment = database_test_environment(&context);

    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        test_db(
            context,
            logger(),
            metrics_registry(),
            environment,
            secrets,
            version,
            test_name,
            DatabaseKind::Redis,
            KubernetesKind::Gke,
            database_mode,
            region,
            is_public,
            ClusterDomain::Default {
                cluster_id: to_short_id(&cluster_id),
            },
            None,
            StorageSize::NormalSize,
        )
    })
}

// Redis self-hosted environment
#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn private_redis_v5_deploy_a_working_dev_environment() {
    test_redis_configuration("5.0", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
#[ignore]
fn public_redis_v5_deploy_a_working_dev_environment() {
    test_redis_configuration("5.0", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn private_redis_v6_deploy_a_working_dev_environment() {
    test_redis_configuration("6.0", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
#[ignore]
fn public_redis_v6_deploy_a_working_dev_environment() {
    test_redis_configuration("6.0", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn private_redis_v7_deploy_a_working_dev_environment() {
    test_redis_configuration("7.0", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn public_redis_v7_deploy_a_working_dev_environment() {
    test_redis_configuration("7.0", function_name!(), CONTAINER, true);
}

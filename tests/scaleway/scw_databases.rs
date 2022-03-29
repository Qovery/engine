use ::function_name::named;
use tracing::{span, warn, Level};

use qovery_engine::cloud_provider::{Kind as ProviderKind, Kind};
use qovery_engine::io_models::{Action, CloneForTest, Database, DatabaseKind, DatabaseMode, Port, Protocol};
use qovery_engine::transaction::TransactionResult;
use test_utilities::utilities::{
    context, engine_run_test, generate_id, generate_password, get_pods, get_svc_name, init, is_pod_restarted_env,
    logger, FuncTestsSecrets,
};

use qovery_engine::io_models::DatabaseMode::{CONTAINER, MANAGED};
use test_utilities::common::test_db;
use test_utilities::common::{database_test_environment, Infrastructure};
use test_utilities::scaleway::{
    clean_environments, scw_default_engine_config, SCW_MANAGED_DATABASE_DISK_TYPE, SCW_MANAGED_DATABASE_INSTANCE_TYPE,
    SCW_SELF_HOSTED_DATABASE_DISK_TYPE, SCW_SELF_HOSTED_DATABASE_INSTANCE_TYPE, SCW_TEST_ZONE,
};

/**
**
** Global database tests
**
**/

// to check overload between several databases and apps
#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn deploy_an_environment_with_3_databases_and_3_apps() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("SCALEWAY_TEST_ORGANIZATION_ID")
                .as_str(),
            secrets
                .SCALEWAY_TEST_CLUSTER_ID
                .as_ref()
                .expect("SCALEWAY_TEST_CLUSTER_ID")
                .as_str(),
        );
        let engine_config = scw_default_engine_config(&context, logger.clone());
        let context_for_deletion = context.clone_not_same_execution_id();
        let engine_config_for_deletion = scw_default_engine_config(&context_for_deletion, logger.clone());
        let environment = test_utilities::common::environment_3_apps_3_routers_3_databases(
            &context,
            secrets
                .clone()
                .DEFAULT_TEST_DOMAIN
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
            SCW_SELF_HOSTED_DATABASE_INSTANCE_TYPE,
            SCW_SELF_HOSTED_DATABASE_DISK_TYPE,
            Kind::Scw,
        );

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;
        let env_action = environment.clone();
        let env_action_delete = environment_delete.clone();

        let result = environment.deploy_environment(&env_action, logger.clone(), &engine_config);
        assert!(matches!(result, TransactionResult::Ok));

        let result = environment_delete.delete_environment(&env_action_delete, logger, &engine_config_for_deletion);
        assert!(matches!(result, TransactionResult::Ok));

        // delete images created during test from registries
        if let Err(e) = clean_environments(&context, vec![environment], secrets, SCW_TEST_ZONE) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn deploy_an_environment_with_db_and_pause_it() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("SCALEWAY_TEST_ORGANIZATION_ID")
                .as_str(),
            secrets
                .SCALEWAY_TEST_CLUSTER_ID
                .as_ref()
                .expect("SCALEWAY_TEST_CLUSTER_ID")
                .as_str(),
        );
        let engine_config = scw_default_engine_config(&context, logger.clone());
        let context_for_deletion = context.clone_not_same_execution_id();
        let engine_config_for_deletion = scw_default_engine_config(&context_for_deletion, logger.clone());
        let environment = test_utilities::common::environnement_2_app_2_routers_1_psql(
            &context,
            secrets
                .clone()
                .DEFAULT_TEST_DOMAIN
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
            SCW_SELF_HOSTED_DATABASE_INSTANCE_TYPE,
            SCW_SELF_HOSTED_DATABASE_DISK_TYPE,
            Kind::Scw,
        );

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;
        let env_action = environment.clone();
        let env_action_delete = environment_delete.clone();

        let result = environment.deploy_environment(&env_action, logger.clone(), &engine_config);
        assert!(matches!(result, TransactionResult::Ok));

        let result = environment.pause_environment(&env_action, logger.clone(), &engine_config);
        assert!(matches!(result, TransactionResult::Ok));

        // Check that we have actually 0 pods running for this db
        let app_name = format!("postgresql{}-0", environment.databases[0].name);
        let ret = get_pods(
            context.clone(),
            ProviderKind::Scw,
            environment.clone(),
            app_name.as_str(),
            secrets.clone(),
        );
        assert_eq!(ret.is_ok(), true);
        assert_eq!(ret.unwrap().items.is_empty(), true);

        let result =
            environment_delete.delete_environment(&env_action_delete, logger.clone(), &engine_config_for_deletion);
        assert!(matches!(result, TransactionResult::Ok));

        // delete images created during test from registries
        if let Err(e) = clean_environments(&context, vec![environment], secrets, SCW_TEST_ZONE) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

// Ensure a full environment can run correctly
#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn postgresql_deploy_a_working_development_environment_with_all_options() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("SCALEWAY_TEST_ORGANIZATION_ID")
                .as_str(),
            secrets
                .SCALEWAY_TEST_CLUSTER_ID
                .as_ref()
                .expect("SCALEWAY_TEST_CLUSTER_ID")
                .as_str(),
        );
        let engine_config = scw_default_engine_config(&context, logger.clone());
        let context_for_deletion = context.clone_not_same_execution_id();
        let engine_config_for_deletion = scw_default_engine_config(&context_for_deletion, logger.clone());
        let test_domain = secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets");

        let environment = test_utilities::common::environnement_2_app_2_routers_1_psql(
            &context,
            test_domain.as_str(),
            SCW_SELF_HOSTED_DATABASE_INSTANCE_TYPE,
            SCW_SELF_HOSTED_DATABASE_DISK_TYPE,
            Kind::Scw,
        );
        let mut environment_delete = test_utilities::common::environnement_2_app_2_routers_1_psql(
            &context_for_deletion,
            test_domain.as_str(),
            SCW_SELF_HOSTED_DATABASE_INSTANCE_TYPE,
            SCW_SELF_HOSTED_DATABASE_DISK_TYPE,
            Kind::Scw,
        );

        environment_delete.action = Action::Delete;

        let env_action = environment.clone();
        let env_action_for_deletion = environment_delete.clone();

        let result = environment.deploy_environment(&env_action, logger.clone(), &engine_config);
        assert!(matches!(result, TransactionResult::Ok));

        let result =
            environment_delete.delete_environment(&env_action_for_deletion, logger, &engine_config_for_deletion);
        assert!(matches!(result, TransactionResult::Ok));

        // delete images created during test from registries
        if let Err(e) =
            clean_environments(&context, vec![environment, environment_delete], secrets.clone(), SCW_TEST_ZONE)
        {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

// Ensure redeploy works as expected
#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn postgresql_deploy_a_working_environment_and_redeploy() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let secrets = FuncTestsSecrets::new();
        let context = context(
            secrets
                .SCALEWAY_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("SCALEWAY_TEST_ORGANIZATION_ID")
                .as_str(),
            secrets
                .SCALEWAY_TEST_CLUSTER_ID
                .as_ref()
                .expect("SCALEWAY_TEST_CLUSTER_ID")
                .as_str(),
        );
        let engine_config = scw_default_engine_config(&context, logger.clone());
        let context_for_redeploy = context.clone_not_same_execution_id();
        let engine_config_for_redeploy = scw_default_engine_config(&context_for_redeploy, logger.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let engine_config_for_delete = scw_default_engine_config(&context_for_delete, logger.clone());

        let mut environment = test_utilities::common::working_minimal_environment(
            &context,
            secrets
                .clone()
                .DEFAULT_TEST_DOMAIN
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

        let app_name = format!("postgresql-app-{}", generate_id());
        let database_mode = CONTAINER;
        let database_host = get_svc_name(DatabaseKind::Postgresql, Kind::Scw).to_string();
        let database_port = 5432;
        let database_db_name = "postgres".to_string();
        let database_username = "superuser".to_string();
        let database_password = generate_password(Kind::Scw, database_mode.clone());

        environment.databases = vec![Database {
            kind: DatabaseKind::Postgresql,
            action: Action::Create,
            id: generate_id(),
            name: database_db_name.clone(),
            version: "11.8.0".to_string(),
            fqdn_id: database_host.clone(),
            fqdn: database_host.clone(),
            port: database_port,
            username: database_username.clone(),
            password: database_password.clone(),
            total_cpus: "500m".to_string(),
            total_ram_in_mib: 512,
            disk_size_in_gib: 10,
            mode: database_mode.clone(),
            database_instance_type: if database_mode == MANAGED {
                SCW_MANAGED_DATABASE_INSTANCE_TYPE
            } else {
                SCW_SELF_HOSTED_DATABASE_INSTANCE_TYPE
            }
            .to_string(),
            database_disk_type: if database_mode == MANAGED {
                SCW_MANAGED_DATABASE_DISK_TYPE
            } else {
                SCW_SELF_HOSTED_DATABASE_DISK_TYPE
            }
            .to_string(),
            encrypt_disk: false,
            activate_high_availability: false,
            activate_backups: false,
            publicly_accessible: false,
        }];
        environment.applications = environment
            .applications
            .into_iter()
            .map(|mut app| {
                app.branch = app_name.clone();
                app.commit_id = "5990752647af11ef21c3d46a51abbde3da1ab351".to_string();
                app.ports = vec![Port {
                    id: "zdf7d6aad".to_string(),
                    long_id: Default::default(),
                    port: 1234,
                    public_port: Some(443),
                    name: None,
                    publicly_accessible: true,
                    protocol: Protocol::HTTP,
                }];
                app.environment_vars = btreemap! {
                     "PG_DBNAME".to_string() => base64::encode(database_db_name.clone()),
                     "PG_HOST".to_string() => base64::encode(database_host.clone()),
                     "PG_PORT".to_string() => base64::encode(database_port.to_string()),
                     "PG_USERNAME".to_string() => base64::encode(database_username.clone()),
                     "PG_PASSWORD".to_string() => base64::encode(database_password.clone()),
                };
                app
            })
            .collect::<Vec<qovery_engine::io_models::Application>>();
        environment.routers[0].routes[0].application_name = app_name;

        let environment_to_redeploy = environment.clone();
        let environment_check = environment.clone();
        let env_action_redeploy = environment_to_redeploy.clone();

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;
        let env_action = environment.clone();
        let env_action_delete = environment_delete.clone();

        let result = environment.deploy_environment(&env_action, logger.clone(), &engine_config);
        assert!(matches!(result, TransactionResult::Ok));

        let result = environment_to_redeploy.deploy_environment(
            &env_action_redeploy,
            logger.clone(),
            &engine_config_for_redeploy,
        );
        assert!(matches!(result, TransactionResult::Ok));

        // TO CHECK: DATABASE SHOULDN'T BE RESTARTED AFTER A REDEPLOY
        let database_name = format!("postgresql-{}-0", &environment_check.databases[0].name);
        match is_pod_restarted_env(
            context.clone(),
            ProviderKind::Scw,
            environment_check,
            database_name.as_str(),
            secrets.clone(),
        ) {
            (true, _) => assert!(true),
            (false, _) => assert!(false),
        }

        let result = environment_delete.delete_environment(&env_action_delete, logger, &engine_config_for_delete);
        assert!(matches!(
            result,
            TransactionResult::Ok | TransactionResult::UnrecoverableError(_, _)
        ));

        // delete images created during test from registries
        if let Err(e) = clean_environments(&context, vec![environment], secrets, SCW_TEST_ZONE) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
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
    let context = context(
        secrets
            .SCALEWAY_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("SCALEWAY_TEST_ORGANIZATION_ID")
            .as_str(),
        secrets
            .SCALEWAY_TEST_CLUSTER_ID
            .as_ref()
            .expect("SCALEWAY_TEST_CLUSTER_ID")
            .as_str(),
    );
    let environment = database_test_environment(&context);

    engine_run_test(|| {
        test_db(
            context,
            logger(),
            environment,
            secrets,
            version,
            test_name,
            DatabaseKind::Postgresql,
            Kind::Scw,
            database_mode,
            is_public,
        )
    })
}

// Postgres self hosted environment
#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn private_postgresql_v10_deploy_a_working_dev_environment() {
    test_postgresql_configuration("10", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
#[ignore]
fn public_postgresql_v10_deploy_a_working_dev_environment() {
    test_postgresql_configuration("10", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn private_postgresql_v11_deploy_a_working_dev_environment() {
    test_postgresql_configuration("11", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
#[ignore]
fn public_postgresql_v11_deploy_a_working_dev_environment() {
    test_postgresql_configuration("11", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn private_postgresql_v12_deploy_a_working_dev_environment() {
    test_postgresql_configuration("12", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
#[ignore]
fn public_postgresql_v12_deploy_a_working_dev_environment() {
    test_postgresql_configuration("12", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn private_postgresql_v13_deploy_a_working_dev_environment() {
    test_postgresql_configuration("13", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn public_postgresql_v13_deploy_a_working_dev_environment() {
    test_postgresql_configuration("13", function_name!(), CONTAINER, true);
}

// Postgres production environment
#[cfg(feature = "test-scw-managed-services")]
#[named]
#[test]
#[ignore]
fn private_postgresql_v10_deploy_a_working_prod_environment() {
    test_postgresql_configuration("10", function_name!(), MANAGED, false);
}

#[cfg(feature = "test-scw-managed-services")]
#[named]
#[test]
#[ignore]
fn public_postgresql_v10_deploy_a_working_prod_environment() {
    test_postgresql_configuration("10", function_name!(), MANAGED, true);
}

#[cfg(feature = "test-scw-managed-services")]
#[named]
#[test]
#[ignore]
fn private_postgresql_v11_deploy_a_working_prod_environment() {
    test_postgresql_configuration("11", function_name!(), MANAGED, false);
}

#[cfg(feature = "test-scw-managed-services")]
#[named]
#[test]
#[ignore]
fn public_postgresql_v11_deploy_a_working_prod_environment() {
    test_postgresql_configuration("11", function_name!(), MANAGED, true);
}

#[cfg(feature = "test-scw-managed-services")]
#[named]
#[test]
#[ignore]
fn private_postgresql_v12_deploy_a_working_prod_environment() {
    test_postgresql_configuration("12", function_name!(), MANAGED, false);
}

#[cfg(feature = "test-scw-managed-services")]
#[named]
#[test]
#[ignore]
fn public_postgresql_v12_deploy_a_working_prod_environment() {
    test_postgresql_configuration("12", function_name!(), MANAGED, true);
}

#[cfg(feature = "test-scw-managed-services")]
#[named]
#[test]
#[ignore]
fn private_postgresql_v13_deploy_a_working_prod_environment() {
    test_postgresql_configuration("13", function_name!(), MANAGED, false);
}

#[cfg(feature = "test-scw-managed-services")]
#[named]
#[test]
#[ignore]
fn public_postgresql_v13_deploy_a_working_prod_environment() {
    test_postgresql_configuration("13", function_name!(), MANAGED, true);
}

/**
 **
 ** MongoDB tests
 **
 **/
#[allow(dead_code)]
fn test_mongodb_configuration(version: &str, test_name: &str, database_mode: DatabaseMode, is_public: bool) {
    let secrets = FuncTestsSecrets::new();
    let context = context(
        secrets
            .SCALEWAY_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("SCALEWAY_TEST_ORGANIZATION_ID")
            .as_str(),
        secrets
            .SCALEWAY_TEST_CLUSTER_ID
            .as_ref()
            .expect("SCALEWAY_TEST_CLUSTER_ID")
            .as_str(),
    );
    let environment = database_test_environment(&context);

    engine_run_test(|| {
        test_db(
            context,
            logger(),
            environment,
            secrets,
            version,
            test_name,
            DatabaseKind::Mongodb,
            Kind::Scw,
            database_mode,
            is_public,
        )
    })
}

// development environment
#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn private_mongodb_v3_6_deploy_a_working_dev_environment() {
    test_mongodb_configuration("3.6", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
#[ignore]
fn public_mongodb_v3_6_deploy_a_working_dev_environment() {
    test_mongodb_configuration("3.6", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn private_mongodb_v4_0_deploy_a_working_dev_environment() {
    test_mongodb_configuration("4.0", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
#[ignore]
fn public_mongodb_v4_0_deploy_a_working_dev_environment() {
    test_mongodb_configuration("4.0", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn private_mongodb_v4_2_deploy_a_working_dev_environment() {
    test_mongodb_configuration("4.2", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
#[ignore]
fn public_mongodb_v4_2_deploy_a_working_dev_environment() {
    test_mongodb_configuration("4.2", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn private_mongodb_v4_4_deploy_a_working_dev_environment() {
    test_mongodb_configuration("4.4", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn public_mongodb_v4_4_deploy_a_working_dev_environment() {
    test_mongodb_configuration("4.4", function_name!(), CONTAINER, true);
}

/**
 **
 ** MySQL tests
 **
 **/
#[allow(dead_code)]
fn test_mysql_configuration(version: &str, test_name: &str, database_mode: DatabaseMode, is_public: bool) {
    let secrets = FuncTestsSecrets::new();
    let context = context(
        secrets
            .SCALEWAY_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("SCALEWAY_TEST_ORGANIZATION_ID")
            .as_str(),
        secrets
            .SCALEWAY_TEST_CLUSTER_ID
            .as_ref()
            .expect("SCALEWAY_TEST_CLUSTER_ID")
            .as_str(),
    );
    let environment = database_test_environment(&context);

    engine_run_test(|| {
        test_db(
            context,
            logger(),
            environment,
            secrets,
            version,
            test_name,
            DatabaseKind::Mysql,
            Kind::Scw,
            database_mode,
            is_public,
        )
    })
}

// MySQL self-hosted environment
#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn private_mysql_v5_7_deploy_a_working_dev_environment() {
    test_mysql_configuration("5.7", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
#[ignore]
fn public_mysql_v5_7_deploy_a_working_dev_environment() {
    test_mysql_configuration("5.7", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn private_mysql_v8_deploy_a_working_dev_environment() {
    test_mysql_configuration("8.0", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn public_mysql_v8_deploy_a_working_dev_environment() {
    test_mysql_configuration("8.0", function_name!(), CONTAINER, true);
}

// MySQL production environment (RDS)
#[cfg(feature = "test-scw-managed-services")]
#[named]
#[test]
#[ignore]
fn private_mysql_v8_deploy_a_working_prod_environment() {
    test_mysql_configuration("8.0", function_name!(), MANAGED, false);
}

#[cfg(feature = "test-scw-managed-services")]
#[named]
#[test]
#[ignore]
fn public_mysql_v8_deploy_a_working_prod_environment() {
    test_mysql_configuration("8.0", function_name!(), MANAGED, true);
}

/**
 **
 ** Redis tests
 **
 **/
#[allow(dead_code)]
fn test_redis_configuration(version: &str, test_name: &str, database_mode: DatabaseMode, is_public: bool) {
    let secrets = FuncTestsSecrets::new();
    let context = context(
        secrets
            .SCALEWAY_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("SCALEWAY_TEST_ORGANIZATION_ID")
            .as_str(),
        secrets
            .SCALEWAY_TEST_CLUSTER_ID
            .as_ref()
            .expect("SCALEWAY_TEST_CLUSTER_ID")
            .as_str(),
    );
    let environment = database_test_environment(&context);

    engine_run_test(|| {
        test_db(
            context,
            logger(),
            environment,
            secrets,
            version,
            test_name,
            DatabaseKind::Redis,
            Kind::Scw,
            database_mode,
            is_public,
        )
    })
}

// Redis self-hosted environment
#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn private_redis_v5_deploy_a_working_dev_environment() {
    test_redis_configuration("5", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
#[ignore]
fn public_redis_v5_deploy_a_working_dev_environment() {
    test_redis_configuration("5", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn private_redis_v6_deploy_a_working_dev_environment() {
    test_redis_configuration("6", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn public_redis_v6_deploy_a_working_dev_environment() {
    test_redis_configuration("6", function_name!(), CONTAINER, true);
}

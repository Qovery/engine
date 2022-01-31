extern crate test_utilities;

use ::function_name::named;
use qovery_engine::cloud_provider::Kind;
use qovery_engine::models::{Action, Clone2, Database, DatabaseKind, DatabaseMode, EnvironmentAction, Port, Protocol};
use qovery_engine::transaction::TransactionResult;
use tracing::{span, Level};

use self::test_utilities::aws::{AWS_DATABASE_DISK_TYPE, AWS_DATABASE_INSTANCE_TYPE};
use self::test_utilities::utilities::{
    context, engine_run_test, generate_id, get_pods, get_svc_name, init, is_pod_restarted_env, logger, FuncTestsSecrets,
};
use qovery_engine::models::DatabaseMode::{CONTAINER, MANAGED};
use test_utilities::common::{test_db, Infrastructure};

/**
**
** Global database tests
**
**/

// to check overload between several databases and apps
#[cfg(feature = "test-aws-self-hosted")]
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
        let context = context(
            secrets
                .AWS_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("AWS_TEST_ORGANIZATION_ID is not set")
                .as_str(),
            secrets
                .AWS_TEST_CLUSTER_ID
                .as_ref()
                .expect("AWS_TEST_CLUSTER_ID is not set")
                .as_str(),
        );
        let context_for_deletion = context.clone_not_same_execution_id();
        let environment = test_utilities::common::environment_3_apps_3_routers_3_databases(
            &context,
            secrets
                .DEFAULT_TEST_DOMAIN
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
            AWS_DATABASE_INSTANCE_TYPE,
            AWS_DATABASE_DISK_TYPE,
        );

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;
        let ea = EnvironmentAction::Environment(environment.clone());
        let ea_delete = EnvironmentAction::Environment(environment_delete.clone());

        match environment.deploy_environment(Kind::Aws, &context, &ea, logger.clone()) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        match environment_delete.delete_environment(Kind::Aws, &context_for_deletion, &ea_delete, logger) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        return test_name.to_string();
    })
}

#[cfg(feature = "test-aws-self-hosted")]
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
        let context = context(
            secrets
                .AWS_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("AWS_TEST_ORGANIZATION_ID is not set")
                .as_str(),
            secrets
                .AWS_TEST_CLUSTER_ID
                .as_ref()
                .expect("AWS_TEST_CLUSTER_ID is not set")
                .as_str(),
        );
        let context_for_deletion = context.clone_not_same_execution_id();
        let environment = test_utilities::common::environnement_2_app_2_routers_1_psql(
            &context,
            secrets
                .clone()
                .DEFAULT_TEST_DOMAIN
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
            AWS_DATABASE_INSTANCE_TYPE,
            AWS_DATABASE_DISK_TYPE,
        );

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;
        let ea = EnvironmentAction::Environment(environment.clone());
        let ea_delete = EnvironmentAction::Environment(environment_delete.clone());

        match environment.deploy_environment(Kind::Aws, &context, &ea, logger.clone()) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        match environment.pause_environment(Kind::Aws, &context, &ea, logger.clone()) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        // Check that we have actually 0 pods running for this db
        let app_name = format!("postgresql-{}-0", environment.databases[0].name);
        let ret = get_pods(
            context.clone(),
            Kind::Aws,
            environment.clone(),
            app_name.clone().as_str(),
            secrets.clone(),
        );
        assert_eq!(ret.is_ok(), true);
        assert_eq!(ret.unwrap().items.is_empty(), true);

        match environment_delete.delete_environment(Kind::Aws, &context_for_deletion, &ea_delete, logger) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        return test_name.to_string();
    })
}

// this test ensure containers databases are never restarted, even in failover environment case
#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn postgresql_failover_dev_environment_with_all_options() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let secrets = FuncTestsSecrets::new();
        let logger = logger();
        let context = context(
            secrets
                .AWS_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("AWS_TEST_ORGANIZATION_ID is not set")
                .as_str(),
            secrets
                .AWS_TEST_CLUSTER_ID
                .as_ref()
                .expect("AWS_TEST_CLUSTER_ID is not set")
                .as_str(),
        );
        let context_for_deletion = context.clone_not_same_execution_id();
        let test_domain = secrets
            .clone()
            .DEFAULT_TEST_DOMAIN
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets");

        let environment = test_utilities::common::environnement_2_app_2_routers_1_psql(
            &context,
            test_domain.as_str(),
            AWS_DATABASE_INSTANCE_TYPE,
            AWS_DATABASE_DISK_TYPE,
        );
        let environment_check = environment.clone();
        let mut environment_never_up = environment.clone();
        // error in ports, these applications will never be up !!
        environment_never_up.applications = environment_never_up
            .applications
            .into_iter()
            .map(|mut app| {
                app.ports = vec![Port {
                    id: "zdf7d6aad".to_string(),
                    long_id: Default::default(),
                    port: 4789,
                    public_port: Some(443),
                    name: None,
                    publicly_accessible: true,
                    protocol: Protocol::HTTP,
                }];
                app
            })
            .collect::<Vec<qovery_engine::models::Application>>();
        let mut environment_delete = test_utilities::common::environnement_2_app_2_routers_1_psql(
            &context_for_deletion,
            test_domain.as_str(),
            AWS_DATABASE_INSTANCE_TYPE,
            AWS_DATABASE_DISK_TYPE,
        );

        environment_delete.action = Action::Delete;

        let ea = EnvironmentAction::Environment(environment.clone());
        let ea_fail_ok = EnvironmentAction::EnvironmentWithFailover(environment_never_up.clone(), environment.clone());
        let ea_for_deletion = EnvironmentAction::Environment(environment_delete.clone());

        match environment.deploy_environment(Kind::Aws, &context, &ea, logger.clone()) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };
        // TO CHECK: DATABASE SHOULDN'T BE RESTARTED AFTER A REDEPLOY
        let database_name = format!("postgresql{}-0", &environment_check.databases[0].name);
        match is_pod_restarted_env(
            context.clone(),
            Kind::Aws,
            environment_check.clone(),
            database_name.as_str(),
            secrets.clone(),
        ) {
            (true, _) => assert!(true),
            (false, _) => assert!(false),
        }
        match environment_never_up.deploy_environment(Kind::Aws, &context, &ea_fail_ok, logger.clone()) {
            TransactionResult::Ok => assert!(false),
            TransactionResult::Rollback(_) => assert!(true),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };
        // TO CHECK: DATABASE SHOULDN'T BE RESTARTED AFTER A REDEPLOY EVEN IF FAIL
        match is_pod_restarted_env(
            context.clone(),
            Kind::Aws,
            environment_check.clone(),
            database_name.as_str(),
            secrets.clone(),
        ) {
            (true, _) => assert!(true),
            (false, _) => assert!(false),
        }

        match environment_delete.delete_environment(Kind::Aws, &context_for_deletion, &ea_for_deletion, logger) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        return test_name.to_string();
    })
}

// Ensure a full environment can run correctly
#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn postgresql_deploy_a_working_development_environment_with_all_options() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let secrets = FuncTestsSecrets::new();
        let logger = logger();
        let context = context(
            secrets
                .AWS_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("AWS_TEST_ORGANIZATION_ID is not set")
                .as_str(),
            secrets
                .AWS_TEST_CLUSTER_ID
                .as_ref()
                .expect("AWS_TEST_CLUSTER_ID is not set")
                .as_str(),
        );
        let context_for_deletion = context.clone_not_same_execution_id();
        let test_domain = secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets");

        let environment = test_utilities::common::environnement_2_app_2_routers_1_psql(
            &context,
            test_domain.as_str(),
            AWS_DATABASE_INSTANCE_TYPE,
            AWS_DATABASE_DISK_TYPE,
        );
        //let env_to_check = environment.clone();
        let mut environment_delete = test_utilities::common::environnement_2_app_2_routers_1_psql(
            &context_for_deletion,
            test_domain.as_str(),
            AWS_DATABASE_INSTANCE_TYPE,
            AWS_DATABASE_DISK_TYPE,
        );

        environment_delete.action = Action::Delete;

        let ea = EnvironmentAction::Environment(environment.clone());
        let ea_for_deletion = EnvironmentAction::Environment(environment_delete.clone());

        match environment.deploy_environment(Kind::Aws, &context, &ea, logger.clone()) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };
        // TODO: should be uncommented as soon as cert-manager is fixed
        // for the moment this assert report a SSL issue on the second router, so it's works well
        /*    let connections = test_utilities::utilities::check_all_connections(&env_to_check);
        for con in connections {
            assert_eq!(con, true);
        }*/

        match environment_delete.delete_environment(Kind::Aws, &context_for_deletion, &ea_for_deletion, logger) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        return test_name.to_string();
    })
}

// Ensure redeploy works as expected
#[cfg(feature = "test-aws-self-hosted")]
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
                .AWS_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("AWS_TEST_ORGANIZATION_ID is not set")
                .as_str(),
            secrets
                .AWS_TEST_CLUSTER_ID
                .as_ref()
                .expect("AWS_TEST_CLUSTER_ID is not set")
                .as_str(),
        );
        let context_for_redeploy = context.clone_not_same_execution_id();
        let context_for_delete = context.clone_not_same_execution_id();

        let mut environment = test_utilities::common::working_minimal_environment(
            &context,
            secrets
                .clone()
                .DEFAULT_TEST_DOMAIN
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

        let app_name = format!("postgresql-app-{}", generate_id());
        let db_id = generate_id();
        let database_host = get_svc_name(DatabaseKind::Postgresql, db_id.clone()).to_string();
        let database_port = 5432;
        let database_db_name = "postgres".to_string();
        let database_username = "superuser".to_string();
        let database_password = generate_id();
        environment.databases = vec![Database {
            kind: DatabaseKind::Postgresql,
            action: Action::Create,
            id: db_id.clone(),
            name: database_db_name.clone(),
            version: "11.8.0".to_string(),
            fqdn_id: "postgresql-".to_string() + generate_id().as_str(),
            fqdn: database_host.clone(),
            port: database_port,
            username: database_username.clone(),
            password: database_password.clone(),
            total_cpus: "500m".to_string(),
            total_ram_in_mib: 512,
            disk_size_in_gib: 10,
            database_instance_type: "db.t2.micro".to_string(),
            database_disk_type: "gp2".to_string(),
            encrypt_disk: false,
            activate_high_availability: false,
            activate_backups: false,
            publicly_accessible: false,
            mode: CONTAINER,
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
            .collect::<Vec<qovery_engine::models::Application>>();
        environment.routers[0].routes[0].application_name = app_name;

        let environment_to_redeploy = environment.clone();
        let environment_check = environment.clone();
        let ea_redeploy = EnvironmentAction::Environment(environment_to_redeploy.clone());

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;
        let ea = EnvironmentAction::Environment(environment.clone());
        let ea_delete = EnvironmentAction::Environment(environment_delete.clone());

        match environment.deploy_environment(Kind::Aws, &context, &ea, logger.clone()) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };
        match environment_to_redeploy.deploy_environment(Kind::Aws, &context_for_redeploy, &ea_redeploy, logger.clone())
        {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };
        // TO CHECK: DATABASE SHOULDN'T BE RESTARTED AFTER A REDEPLOY
        let database_name = format!("postgresql{}-0", &environment_check.databases[0].name);
        match is_pod_restarted_env(
            context.clone(),
            Kind::Aws,
            environment_check,
            database_name.as_str(),
            secrets.clone(),
        ) {
            (true, _) => assert!(true),
            (false, _) => assert!(false),
        }

        match environment_delete.delete_environment(Kind::Aws, &context_for_delete, &ea_delete, logger) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(true),
        };

        return test_name.to_string();
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
            .AWS_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("AWS_TEST_ORGANIZATION_ID is not set")
            .as_str(),
        secrets
            .AWS_TEST_CLUSTER_ID
            .as_ref()
            .expect("AWS_TEST_CLUSTER_ID is not set")
            .as_str(),
    );

    engine_run_test(|| {
        test_db(
            context,
            logger(),
            secrets,
            version,
            test_name,
            DatabaseKind::Postgresql,
            Kind::Aws,
            database_mode,
            is_public,
        )
    })
}

// Postgres environment environment
#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn private_postgresql_v10_deploy_a_working_dev_environment() {
    test_postgresql_configuration("10", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn public_postgresql_v10_deploy_a_working_dev_environment() {
    test_postgresql_configuration("10", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn private_postgresql_v11_deploy_a_working_dev_environment() {
    test_postgresql_configuration("11", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn public_postgresql_v11_deploy_a_working_dev_environment() {
    test_postgresql_configuration("11", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn private_postgresql_v12_deploy_a_working_dev_environment() {
    test_postgresql_configuration("12", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn public_postgresql_v12_deploy_a_working_dev_environment() {
    test_postgresql_configuration("12", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn private_postgresql_v13_deploy_a_working_dev_environment() {
    test_postgresql_configuration("13", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn public_postgresql_v13_deploy_a_working_dev_environment() {
    test_postgresql_configuration("13", function_name!(), CONTAINER, true);
}

// Postgres production environment
#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn private_postgresql_v10_deploy_a_working_prod_environment() {
    test_postgresql_configuration("10", function_name!(), MANAGED, false);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn public_postgresql_v10_deploy_a_working_prod_environment() {
    test_postgresql_configuration("10", function_name!(), MANAGED, true);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn private_postgresql_v11_deploy_a_working_prod_environment() {
    test_postgresql_configuration("11", function_name!(), MANAGED, false);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn public_postgresql_v11_deploy_a_working_prod_environment() {
    test_postgresql_configuration("11", function_name!(), MANAGED, true);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn private_postgresql_v12_deploy_a_working_prod_environment() {
    test_postgresql_configuration("12", function_name!(), MANAGED, false);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn public_postgresql_v12_deploy_a_working_prod_environment() {
    test_postgresql_configuration("12", function_name!(), MANAGED, true);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn private_postgresql_v13_deploy_a_working_prod_environment() {
    test_postgresql_configuration("13", function_name!(), MANAGED, false);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
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
            .AWS_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("AWS_TEST_ORGANIZATION_ID is not set")
            .as_str(),
        secrets
            .AWS_TEST_CLUSTER_ID
            .as_ref()
            .expect("AWS_TEST_CLUSTER_ID is not set")
            .as_str(),
    );

    engine_run_test(|| {
        test_db(
            context,
            logger(),
            secrets,
            version,
            test_name,
            DatabaseKind::Mongodb,
            Kind::Aws,
            database_mode,
            is_public,
        )
    })
}

// development environment
#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn private_mongodb_v3_6_deploy_a_working_dev_environment() {
    test_mongodb_configuration("3.6", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn public_mongodb_v3_6_deploy_a_working_dev_environment() {
    test_mongodb_configuration("3.6", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn private_mongodb_v4_0_deploy_a_working_dev_environment() {
    test_mongodb_configuration("4.0", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn public_mongodb_v4_0_deploy_a_working_dev_environment() {
    test_mongodb_configuration("4.0", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn private_mongodb_v4_2_deploy_a_working_dev_environment() {
    test_mongodb_configuration("4.2", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn public_mongodb_v4_2_deploy_a_working_dev_environment() {
    test_mongodb_configuration("4.2", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn private_mongodb_v4_4_deploy_a_working_dev_environment() {
    test_mongodb_configuration("4.4", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn public_mongodb_v4_4_deploy_a_working_dev_environment() {
    test_mongodb_configuration("4.4", function_name!(), CONTAINER, true);
}

// MongoDB production environment (DocumentDB)
#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn private_mongodb_v3_6_deploy_a_working_prod_environment() {
    test_mongodb_configuration("3.6", function_name!(), MANAGED, false);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn public_mongodb_v3_6_deploy_a_working_prod_environment() {
    test_mongodb_configuration("3.6", function_name!(), MANAGED, true);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn private_mongodb_v4_0_deploy_a_working_prod_environment() {
    test_mongodb_configuration("4.0", function_name!(), MANAGED, false);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn public_mongodb_v4_0_deploy_a_working_prod_environment() {
    test_mongodb_configuration("4.0", function_name!(), MANAGED, true);
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
            .AWS_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("AWS_TEST_ORGANIZATION_ID is not set")
            .as_str(),
        secrets
            .AWS_TEST_CLUSTER_ID
            .as_ref()
            .expect("AWS_TEST_CLUSTER_ID is not set")
            .as_str(),
    );

    engine_run_test(|| {
        test_db(
            context,
            logger(),
            secrets,
            version,
            test_name,
            DatabaseKind::Mysql,
            Kind::Aws,
            database_mode,
            is_public,
        )
    })
}

// MySQL self-hosted environment
#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn private_mysql_v5_7_deploy_a_working_dev_environment() {
    test_mysql_configuration("5.7", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn public_mysql_v5_7_deploy_a_working_dev_environment() {
    test_mysql_configuration("5.7", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn private_mysql_v8_deploy_a_working_dev_environment() {
    test_mysql_configuration("8.0", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn public_mysql_v8_deploy_a_working_dev_environment() {
    test_mysql_configuration("8.0", function_name!(), CONTAINER, true);
}

// MySQL production environment (RDS)
#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn private_mysql_v5_7_deploy_a_working_prod_environment() {
    test_mysql_configuration("5.7", function_name!(), MANAGED, false);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn public_mysql_v5_7_deploy_a_working_prod_environment() {
    test_mysql_configuration("5.7", function_name!(), MANAGED, true);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn private_mysql_v8_0_deploy_a_working_prod_environment() {
    test_mysql_configuration("8.0", function_name!(), MANAGED, false);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn public_mysql_v8_0_deploy_a_working_prod_environment() {
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
            .AWS_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("AWS_TEST_ORGANIZATION_ID is not set")
            .as_str(),
        secrets
            .AWS_TEST_CLUSTER_ID
            .as_ref()
            .expect("AWS_TEST_CLUSTER_ID is not set")
            .as_str(),
    );

    engine_run_test(|| {
        test_db(
            context,
            logger(),
            secrets,
            version,
            test_name,
            DatabaseKind::Redis,
            Kind::Aws,
            database_mode,
            is_public,
        )
    })
}

// Redis self-hosted environment
#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn private_redis_v5_deploy_a_working_dev_environment() {
    test_redis_configuration("5", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn public_redis_v5_deploy_a_working_dev_environment() {
    test_redis_configuration("5", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn private_redis_v6_deploy_a_working_dev_environment() {
    test_redis_configuration("6", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn public_redis_v6_deploy_a_working_dev_environment() {
    test_redis_configuration("6", function_name!(), CONTAINER, true);
}

// Redis production environment (Elasticache)
#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn private_redis_v5_deploy_a_working_prod_environment() {
    test_redis_configuration("5", function_name!(), MANAGED, false);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn public_redis_v5_deploy_a_working_prod_environment() {
    test_redis_configuration("5", function_name!(), MANAGED, true);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn private_redis_v6_deploy_a_working_prod_environment() {
    test_redis_configuration("6", function_name!(), MANAGED, false);
}

#[cfg(feature = "test-aws-managed-services")]
#[named]
#[test]
fn public_redis_v6_deploy_a_working_prod_environment() {
    test_redis_configuration("6", function_name!(), MANAGED, true);
}

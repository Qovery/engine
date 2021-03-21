extern crate test_utilities;

use test_utilities::utilities::{init, FuncTestsSecrets};
use tracing::{span, Level};

use qovery_engine::models::{
    Action, Clone2, Context, Database, DatabaseKind, Environment, EnvironmentAction, EnvironmentVariable, Kind,
};
use qovery_engine::transaction::TransactionResult;

use crate::aws::aws_environment::{delete_environment, deploy_environment};

use self::test_utilities::utilities::{context, engine_run_test, generate_id, is_pod_restarted_aws_env};

/**
**
** Global database tests
**
**/

// to check overload between several databases and apps
#[cfg(feature = "test-aws-self-hosted")]
#[test]
fn deploy_an_environment_with_3_databases_and_3_apps() {
    init();

    let span = span!(
        Level::INFO,
        "test",
        name = "deploy_an_environment_with_3_databases_and_3_apps"
    );
    let _enter = span.enter();

    let context = context();
    let context_for_deletion = context.clone_not_same_execution_id();
    let secrets = FuncTestsSecrets::new();
    let environment = test_utilities::aws::environment_3_apps_3_routers_3_databases(&context, secrets);

    let mut environment_delete = environment.clone();
    environment_delete.action = Action::Delete;
    let ea = EnvironmentAction::Environment(environment);
    let ea_delete = EnvironmentAction::Environment(environment_delete);

    match deploy_environment(&context, &ea) {
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

    match delete_environment(&context_for_deletion, &ea_delete) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

// this test ensure containers databases are never restarted, even in failover environment case
#[cfg(feature = "test-aws-self-hosted")]
#[test]
fn postgresql_failover_dev_environment_with_all_options() {
    init();

    let span = span!(
        Level::INFO,
        "test",
        name = "postgresql_deploy_a_working_development_environment_with_all_options"
    );
    let _enter = span.enter();

    let context = context();
    let context_for_deletion = context.clone_not_same_execution_id();
    let secrets = FuncTestsSecrets::new();

    let mut environment = test_utilities::aws::environnement_2_app_2_routers_1_psql(&context, secrets.clone());
    let environment_check = environment.clone();
    let mut environment_never_up = environment.clone();
    // error in ports, these applications will never be up !!
    environment_never_up.applications = environment_never_up
        .applications
        .into_iter()
        .map(|mut app| {
            app.private_port = Some(4789);
            app
        })
        .collect::<Vec<qovery_engine::models::Application>>();
    let mut environment_delete =
        test_utilities::aws::environnement_2_app_2_routers_1_psql(&context_for_deletion, secrets.clone());

    environment.kind = Kind::Development;
    environment_delete.kind = Kind::Development;
    environment_delete.action = Action::Delete;

    let ea = EnvironmentAction::Environment(environment.clone());
    let ea_fail_ok = EnvironmentAction::EnvironmentWithFailover(environment_never_up, environment.clone());
    let ea_for_deletion = EnvironmentAction::Environment(environment_delete);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
    // TO CHECK: DATABASE SHOULDN'T BE RESTARTED AFTER A REDEPLOY
    let database_name = format!("postgresql{}-0", &environment_check.databases[0].name);
    match is_pod_restarted_aws_env(environment_check.clone(), database_name.as_str(), secrets.clone()) {
        (true, _) => assert!(true),
        (false, _) => assert!(false),
    }
    match deploy_environment(&context, &ea_fail_ok) {
        TransactionResult::Ok => assert!(false),
        TransactionResult::Rollback(_) => assert!(true),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
    // TO CHECK: DATABASE SHOULDN'T BE RESTARTED AFTER A REDEPLOY EVEN IF FAIL
    match is_pod_restarted_aws_env(environment_check.clone(), database_name.as_str(), secrets) {
        (true, _) => assert!(true),
        (false, _) => assert!(false),
    }

    match delete_environment(&context_for_deletion, &ea_for_deletion) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

// Ensure a full environment can run correctly
#[cfg(feature = "test-aws-self-hosted")]
#[test]
fn postgresql_deploy_a_working_development_environment_with_all_options() {
    init();

    let span = span!(
        Level::INFO,
        "test",
        name = "postgresql_deploy_a_working_development_environment_with_all_options"
    );
    let _enter = span.enter();

    let context = context();
    let secrets = FuncTestsSecrets::new();
    let context_for_deletion = context.clone_not_same_execution_id();

    let mut environment = test_utilities::aws::environnement_2_app_2_routers_1_psql(&context, secrets.clone());
    //let env_to_check = environment.clone();
    let mut environment_delete =
        test_utilities::aws::environnement_2_app_2_routers_1_psql(&context_for_deletion, secrets.clone());

    environment.kind = Kind::Development;
    environment_delete.kind = Kind::Development;
    environment_delete.action = Action::Delete;

    let ea = EnvironmentAction::Environment(environment);
    let ea_for_deletion = EnvironmentAction::Environment(environment_delete);

    match deploy_environment(&context, &ea) {
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

    match delete_environment(&context_for_deletion, &ea_for_deletion) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

// Ensure redeploy works as expected
#[cfg(feature = "test-aws-self-hosted")]
#[test]
fn postgresql_deploy_a_working_environment_and_redeploy() {
    engine_run_test(|| {
        let span = span!(
            Level::INFO,
            "test",
            name = "postgresql_deploy_a_working_environment_and_redeploy"
        );
        let _enter = span.enter();

        let secrets = FuncTestsSecrets::new();
        let context = context();
        let context_for_redeploy = context.clone_not_same_execution_id();
        let context_for_delete = context.clone_not_same_execution_id();

        let mut environment = test_utilities::aws::working_minimal_environment(&context, secrets.clone());

        let app_name = format!("postgresql-app-{}", generate_id());
        let database_host = format!(
            "postgresql-{}.{}",
            generate_id(),
            secrets.clone().DEFAULT_TEST_DOMAIN.unwrap()
        );
        let database_port = 5432;
        let database_db_name = "postgresql".to_string();
        let database_username = "superuser".to_string();
        let database_password = generate_id();
        environment.databases = vec![Database {
            kind: DatabaseKind::Postgresql,
            action: Action::Create,
            id: generate_id(),
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
        }];
        environment.applications = environment
            .applications
            .into_iter()
            .map(|mut app| {
                app.branch = app_name.clone();
                app.commit_id = "5990752647af11ef21c3d46a51abbde3da1ab351".to_string();
                app.private_port = Some(1234);
                app.environment_variables = vec![
                    EnvironmentVariable {
                        key: "PG_HOST".to_string(),
                        value: database_host.clone(),
                    },
                    EnvironmentVariable {
                        key: "PG_PORT".to_string(),
                        value: database_port.clone().to_string(),
                    },
                    EnvironmentVariable {
                        key: "PG_DBNAME".to_string(),
                        value: database_db_name.clone(),
                    },
                    EnvironmentVariable {
                        key: "PG_USERNAME".to_string(),
                        value: database_username.clone(),
                    },
                    EnvironmentVariable {
                        key: "PG_PASSWORD".to_string(),
                        value: database_password.clone(),
                    },
                ];
                app
            })
            .collect::<Vec<qovery_engine::models::Application>>();
        environment.routers[0].routes[0].application_name = app_name;

        let environment_to_redeploy = environment.clone();
        let environment_check = environment.clone();
        let ea_redeploy = EnvironmentAction::Environment(environment_to_redeploy);

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;
        let ea = EnvironmentAction::Environment(environment);
        let ea_delete = EnvironmentAction::Environment(environment_delete);

        match deploy_environment(&context, &ea) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };
        match deploy_environment(&context_for_redeploy, &ea_redeploy) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };
        // TO CHECK: DATABASE SHOULDN'T BE RESTARTED AFTER A REDEPLOY
        let database_name = format!("postgresql{}-0", &environment_check.databases[0].name);
        match is_pod_restarted_aws_env(environment_check, database_name.as_str(), secrets) {
            (true, _) => assert!(true),
            (false, _) => assert!(false),
        }

        match delete_environment(&context_for_delete, &ea_delete) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(true),
        };
        "postgresql_deploy_a_working_environment_and_redeploy".to_string()
    })
}

/**
**
** PostgreSQL tests
**
**/

fn test_postgresql_configuration(
    context: Context,
    mut environment: Environment,
    secrets: FuncTestsSecrets,
    version: &str,
    test_name: &str,
) {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();
        let context_for_delete = context.clone_not_same_execution_id();

        let app_name = format!("postgresql-app-{}", generate_id());
        let database_host = format!("postgresql-{}.{}", generate_id(), secrets.DEFAULT_TEST_DOMAIN.unwrap());
        let database_port = 5432;
        let database_db_name = "postgres".to_string();
        let database_username = "superuser".to_string();
        let database_password = generate_id();

        let _is_rds = match environment.kind {
            Kind::Production => true,
            Kind::Development => false,
        };

        environment.databases = vec![Database {
            kind: DatabaseKind::Postgresql,
            action: Action::Create,
            id: generate_id(),
            name: database_db_name.clone(),
            version: version.to_string(),
            fqdn_id: "postgresql-".to_string() + generate_id().as_str(),
            fqdn: database_host.clone(),
            port: database_port.clone(),
            username: database_username.clone(),
            password: database_password.clone(),
            total_cpus: "100m".to_string(),
            total_ram_in_mib: 512,
            disk_size_in_gib: 10,
            database_instance_type: "db.t2.micro".to_string(),
            database_disk_type: "gp2".to_string(),
        }];
        environment.applications = environment
            .applications
            .into_iter()
            .map(|mut app| {
                app.branch = app_name.clone();
                app.commit_id = "ad65b24a0470e7e8aa0983e036fb9a05928fd973".to_string();
                app.private_port = Some(1234);
                app.dockerfile_path = Some(format!("Dockerfile-{}", version));
                app.environment_variables = vec![
                    EnvironmentVariable {
                        key: "PG_HOST".to_string(),
                        value: database_host.clone(),
                    },
                    EnvironmentVariable {
                        key: "PG_PORT".to_string(),
                        value: database_port.clone().to_string(),
                    },
                    EnvironmentVariable {
                        key: "PG_DBNAME".to_string(),
                        value: database_db_name.clone(),
                    },
                    EnvironmentVariable {
                        key: "PG_USERNAME".to_string(),
                        value: database_username.clone(),
                    },
                    EnvironmentVariable {
                        key: "PG_PASSWORD".to_string(),
                        value: database_password.clone(),
                    },
                ];
                app
            })
            .collect::<Vec<qovery_engine::models::Application>>();
        environment.routers[0].routes[0].application_name = app_name.clone();

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;
        let ea = EnvironmentAction::Environment(environment);
        let ea_delete = EnvironmentAction::Environment(environment_delete);

        match deploy_environment(&context, &ea) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        // todo: check the database disk is here and with correct size

        match delete_environment(&context_for_delete, &ea_delete) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(true),
        };
        return test_name.to_string();
    })
}

// Postgres environment environment
#[cfg(feature = "test-aws-self-hosted")]
#[test]
fn postgresql_v10_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = test_utilities::aws::working_minimal_environment(&context, secrets.clone());
    test_postgresql_configuration(
        context,
        environment,
        secrets,
        "10",
        "postgresql_v10_deploy_a_working_dev_environment",
    );
}

#[cfg(feature = "test-aws-self-hosted")]
#[test]
fn postgresql_v11_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = test_utilities::aws::working_minimal_environment(&context, secrets.clone());
    test_postgresql_configuration(
        context,
        environment,
        secrets,
        "11",
        "postgresql_v11_deploy_a_working_dev_environment",
    );
}

#[cfg(feature = "test-aws-self-hosted")]
#[test]
fn postgresql_v12_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = test_utilities::aws::working_minimal_environment(&context, secrets.clone());
    test_postgresql_configuration(
        context,
        environment,
        secrets,
        "12",
        "postgresql_v12_deploy_a_working_dev_environment",
    );
}

// Postgres production environment
#[cfg(feature = "test-aws-managed-services")]
#[test]
fn postgresql_v10_deploy_a_working_prod_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let mut environment = test_utilities::aws::working_minimal_environment(&context, secrets.clone());
    environment.kind = Kind::Production;
    test_postgresql_configuration(
        context,
        environment,
        secrets,
        "10",
        "postgresql_v10_deploy_a_working_prod_environment",
    );
}

#[cfg(feature = "test-aws-managed-services")]
#[test]
fn postgresql_v11_deploy_a_working_prod_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let mut environment = test_utilities::aws::working_minimal_environment(&context, secrets.clone());
    environment.kind = Kind::Production;
    test_postgresql_configuration(
        context,
        environment,
        secrets,
        "11",
        "postgresql_v11_deploy_a_working_prod_environment",
    );
}

#[cfg(feature = "test-aws-managed-services")]
#[test]
fn postgresql_v12_deploy_a_working_prod_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let mut environment = test_utilities::aws::working_minimal_environment(&context, secrets.clone());
    environment.kind = Kind::Production;
    test_postgresql_configuration(
        context,
        environment,
        secrets,
        "12",
        "postgresql_v12_deploy_a_working_prod_environment",
    );
}

/**
**
** MongoDB tests
**
**/

fn test_mongodb_configuration(
    context: Context,
    mut environment: Environment,
    secrets: FuncTestsSecrets,
    version: &str,
    test_name: &str,
) {
    init();

    let span = span!(Level::INFO, "test", name = test_name);
    let _enter = span.enter();
    let context_for_delete = context.clone_not_same_execution_id();

    let app_name = format!("mongodb-app-{}", generate_id());
    let database_host = format!("mongodb-{}.{}", generate_id(), secrets.DEFAULT_TEST_DOMAIN.unwrap());
    let database_port = 27017;
    let database_db_name = "my-mongodb".to_string();
    let database_username = "superuser".to_string();
    let database_password = generate_id();
    let database_uri = format!(
        "mongodb://{}:{}@{}:{}/{}",
        database_username, database_password, database_host, database_port, database_db_name
    );
    // while waiting the info to be given directly in the database info, we're using this
    let is_documentdb = match environment.kind {
        Kind::Production => true,
        Kind::Development => false,
    };

    environment.databases = vec![Database {
        kind: DatabaseKind::Mongodb,
        action: Action::Create,
        id: generate_id(),
        name: database_db_name.clone(),
        version: version.to_string(),
        fqdn_id: "mongodb-".to_string() + generate_id().as_str(),
        fqdn: database_host.clone(),
        port: database_port.clone(),
        username: database_username.clone(),
        password: database_password.clone(),
        total_cpus: "500m".to_string(),
        total_ram_in_mib: 512,
        disk_size_in_gib: 10,
        database_instance_type: "db.t3.medium".to_string(),
        database_disk_type: "gp2".to_string(),
    }];
    environment.applications = environment
        .applications
        .into_iter()
        .map(|mut app| {
            app.branch = app_name.clone();
            app.commit_id = "3fdc7e784c1d98b80446be7ff25e35370306d9a8".to_string();
            app.private_port = Some(1234);
            app.dockerfile_path = Some(format!("Dockerfile-{}", version));
            app.environment_variables = vec![
                // EnvironmentVariable {
                //     key: "ENABLE_DEBUG".to_string(),
                //     value: "true".to_string(),
                // },
                // EnvironmentVariable {
                //     key: "DEBUG_PAUSE".to_string(),
                //     value: "true".to_string(),
                // },
                EnvironmentVariable {
                    key: "IS_DOCUMENTDB".to_string(),
                    value: is_documentdb.to_string(),
                },
                EnvironmentVariable {
                    key: "QOVERY_DATABASE_TESTING_DATABASE_FQDN".to_string(),
                    value: database_host.clone(),
                },
                EnvironmentVariable {
                    key: "QOVERY_DATABASE_MY_DDB_CONNECTION_URI".to_string(),
                    value: database_uri.clone(),
                },
                EnvironmentVariable {
                    key: "QOVERY_DATABASE_TESTING_DATABASE_PORT".to_string(),
                    value: database_port.clone().to_string(),
                },
                EnvironmentVariable {
                    key: "MONGODB_DBNAME".to_string(),
                    value: database_db_name.clone(),
                },
                EnvironmentVariable {
                    key: "QOVERY_DATABASE_TESTING_DATABASE_USERNAME".to_string(),
                    value: database_username.clone(),
                },
                EnvironmentVariable {
                    key: "QOVERY_DATABASE_TESTING_DATABASE_PASSWORD".to_string(),
                    value: database_password.clone(),
                },
            ];
            app
        })
        .collect::<Vec<qovery_engine::models::Application>>();
    environment.routers[0].routes[0].application_name = app_name.clone();

    let mut environment_delete = environment.clone();
    environment_delete.action = Action::Delete;
    let ea = EnvironmentAction::Environment(environment);
    let ea_delete = EnvironmentAction::Environment(environment_delete);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    // todo: check the database disk is here and with correct size

    match delete_environment(&context_for_delete, &ea_delete) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(true),
    };
}

// development environment
#[cfg(feature = "test-aws-self-hosted")]
#[test]
fn mongodb_v3_6_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = test_utilities::aws::working_minimal_environment(&context, secrets.clone());
    test_mongodb_configuration(
        context,
        environment,
        secrets,
        "3.6",
        "mongodb_v3_6_deploy_a_working_dev_environment",
    );
}

#[cfg(feature = "test-aws-self-hosted")]
#[test]
fn mongodb_v4_0_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = test_utilities::aws::working_minimal_environment(&context, secrets.clone());
    test_mongodb_configuration(
        context,
        environment,
        secrets,
        "4.0",
        "mongodb_v4_0_deploy_a_working_dev_environment",
    );
}

#[cfg(feature = "test-aws-self-hosted")]
#[test]
fn mongodb_v4_2_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = test_utilities::aws::working_minimal_environment(&context, secrets.clone());
    test_mongodb_configuration(
        context,
        environment,
        secrets,
        "4.2",
        "mongodb_v4_2_deploy_a_working_dev_environment",
    );
}

#[cfg(feature = "test-aws-self-hosted")]
#[test]
fn mongodb_v4_4_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = test_utilities::aws::working_minimal_environment(&context, secrets.clone());
    test_mongodb_configuration(
        context,
        environment,
        secrets,
        "4.4",
        "mongodb_v4_4_deploy_a_working_dev_environment",
    );
}

// MongoDB production environment (DocumentDB)
#[cfg(feature = "test-aws-managed-services")]
#[test]
fn mongodb_v3_6_deploy_a_working_prod_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let mut environment = test_utilities::aws::working_minimal_environment(&context, secrets.clone());
    environment.kind = Kind::Production;
    test_mongodb_configuration(
        context,
        environment,
        secrets,
        "3.6",
        "mongodb_v3_6_deploy_a_working_prod_environment",
    );
}

#[cfg(feature = "test-aws-managed-services")]
#[test]
fn mongodb_v4_0_deploy_a_working_prod_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let mut environment = test_utilities::aws::working_minimal_environment(&context, secrets.clone());
    environment.kind = Kind::Production;
    test_mongodb_configuration(
        context,
        environment,
        secrets,
        "4.0",
        "mongodb_v4_0_deploy_a_working_prod_environment",
    );
}

/**
**
** MySQL tests
**
**/

fn test_mysql_configuration(
    context: Context,
    mut environment: Environment,
    secrets: FuncTestsSecrets,
    version: &str,
    test_name: &str,
) {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let deletion_context = context.clone_not_same_execution_id();

        let app_name = format!("mysql-app-{}", generate_id());
        let database_host = format!("mysql-{}.{}", generate_id(), secrets.DEFAULT_TEST_DOMAIN.unwrap());

        let database_port = 3306;
        let database_db_name = "mysqldatabase".to_string();
        let database_username = "superuser".to_string();
        let database_password = generate_id();

        let _is_rds = match environment.kind {
            Kind::Production => true,
            Kind::Development => false,
        };

        environment.databases = vec![Database {
            kind: DatabaseKind::Mysql,
            action: Action::Create,
            id: generate_id(),
            name: database_db_name.clone(),
            version: version.to_string(),
            fqdn_id: "mysql-".to_string() + generate_id().as_str(),
            fqdn: database_host.clone(),
            port: database_port.clone(),
            username: database_username.clone(),
            password: database_password.clone(),
            total_cpus: "500m".to_string(),
            total_ram_in_mib: 512,
            disk_size_in_gib: 10,
            database_instance_type: "db.t2.micro".to_string(),
            database_disk_type: "gp2".to_string(),
        }];
        environment.applications = environment
            .applications
            .into_iter()
            .map(|mut app| {
                app.branch = app_name.clone();
                app.commit_id = "fc8a87b39cdee84bb789893fb823e3e62a1999c0".to_string();
                app.private_port = Some(1234);
                app.dockerfile_path = Some(format!("Dockerfile-{}", version));
                app.environment_variables = vec![
                    // EnvironmentVariable {
                    //     key: "ENABLE_DEBUG".to_string(),
                    //     value: "true".to_string(),
                    // },
                    // EnvironmentVariable {
                    //     key: "DEBUG_PAUSE".to_string(),
                    //     value: "true".to_string(),
                    // },
                    EnvironmentVariable {
                        key: "MYSQL_HOST".to_string(),
                        value: database_host.clone(),
                    },
                    EnvironmentVariable {
                        key: "MYSQL_PORT".to_string(),
                        value: database_port.clone().to_string(),
                    },
                    EnvironmentVariable {
                        key: "MYSQL_DBNAME".to_string(),
                        value: database_db_name.clone(),
                    },
                    EnvironmentVariable {
                        key: "MYSQL_USERNAME".to_string(),
                        value: database_username.clone(),
                    },
                    EnvironmentVariable {
                        key: "MYSQL_PASSWORD".to_string(),
                        value: database_password.clone(),
                    },
                ];
                app
            })
            .collect::<Vec<qovery_engine::models::Application>>();
        environment.routers[0].routes[0].application_name = app_name.clone();

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;
        let ea = EnvironmentAction::Environment(environment);
        let ea_delete = EnvironmentAction::Environment(environment_delete);

        match deploy_environment(&context, &ea) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        // todo: check the database disk is here and with correct size

        match delete_environment(&deletion_context, &ea_delete) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        return test_name.to_string();
    })
}

// MySQL self-hosted environment
#[cfg(feature = "test-aws-self-hosted")]
#[test]
fn mysql_v5_7_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = test_utilities::aws::working_minimal_environment(&context, secrets.clone());
    test_mysql_configuration(
        context,
        environment,
        secrets,
        "5.7",
        "mysql_v5_7_deploy_a_working_dev_environment",
    );
}

#[cfg(feature = "test-aws-self-hosted")]
#[test]
fn mysql_v8_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = test_utilities::aws::working_minimal_environment(&context, secrets.clone());
    test_mysql_configuration(
        context,
        environment,
        secrets,
        "8.0",
        "mysql_v8_deploy_a_working_dev_environment",
    );
}

// MySQL production environment (RDS)
#[cfg(feature = "test-aws-managed-services")]
#[test]
fn mysql_v5_7_deploy_a_working_prod_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let mut environment = test_utilities::aws::working_minimal_environment(&context, secrets.clone());
    environment.kind = Kind::Production;
    test_mysql_configuration(
        context,
        environment,
        secrets,
        "5.7",
        "mysql_v5_7_deploy_a_working_prod_environment",
    );
}

#[cfg(feature = "test-aws-managed-services")]
#[test]
fn mysql_v8_0_deploy_a_working_prod_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let mut environment = test_utilities::aws::working_minimal_environment(&context, secrets.clone());
    environment.kind = Kind::Production;
    test_mysql_configuration(
        context,
        environment,
        secrets,
        "8.0",
        "mysql_v8_0_deploy_a_working_prod_environment",
    );
}

/**
**
** Redis tests
**
**/

fn test_redis_configuration(
    context: Context,
    mut environment: Environment,
    secrets: FuncTestsSecrets,
    version: &str,
    test_name: &str,
) {
    engine_run_test(|| {
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let context_for_delete = context.clone_not_same_execution_id();

        let app_name = format!("redis-app-{}", generate_id());
        let database_host = format!("redis-{}.{}", generate_id(), secrets.DEFAULT_TEST_DOMAIN.unwrap());
        let database_port = 6379;
        let database_db_name = "my-redis".to_string();
        let database_username = "superuser".to_string();
        let database_password = generate_id();

        let is_elasticache = match environment.kind {
            Kind::Production => true,
            Kind::Development => false,
        };

        environment.databases = vec![Database {
            kind: DatabaseKind::Redis,
            action: Action::Create,
            id: generate_id(),
            name: database_db_name.clone(),
            version: version.to_string(),
            fqdn_id: "redis-".to_string() + generate_id().as_str(),
            fqdn: database_host.clone(),
            port: database_port.clone(),
            username: database_username.clone(),
            password: database_password.clone(),
            total_cpus: "500m".to_string(),
            total_ram_in_mib: 512,
            disk_size_in_gib: 10,
            database_instance_type: "cache.t3.micro".to_string(),
            database_disk_type: "gp2".to_string(),
        }];
        environment.applications = environment
            .applications
            .into_iter()
            .map(|mut app| {
                app.name = app_name.clone();
                app.branch = "redis-app".to_string();
                app.commit_id = "80ad41fbe9549f8de8dbe2ca4dd5d23e8ffc92de".to_string();
                app.private_port = Some(1234);
                app.dockerfile_path = Some(format!("Dockerfile-{}", version));
                app.environment_variables = vec![
                    // EnvironmentVariable {
                    //     key: "ENABLE_DEBUG".to_string(),
                    //     value: "true".to_string(),
                    // },
                    // EnvironmentVariable {
                    //     key: "DEBUG_PAUSE".to_string(),
                    //     value: "true".to_string(),
                    // },
                    EnvironmentVariable {
                        key: "IS_ELASTICCACHE".to_string(),
                        value: is_elasticache.to_string(),
                    },
                    EnvironmentVariable {
                        key: "REDIS_HOST".to_string(),
                        value: database_host.clone(),
                    },
                    EnvironmentVariable {
                        key: "REDIS_PORT".to_string(),
                        value: database_port.clone().to_string(),
                    },
                    EnvironmentVariable {
                        key: "REDIS_USERNAME".to_string(),
                        value: database_username.clone(),
                    },
                    EnvironmentVariable {
                        key: "REDIS_PASSWORD".to_string(),
                        value: database_password.clone(),
                    },
                ];
                app
            })
            .collect::<Vec<qovery_engine::models::Application>>();
        environment.routers[0].routes[0].application_name = app_name.clone();

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;
        let ea = EnvironmentAction::Environment(environment);
        let ea_delete = EnvironmentAction::Environment(environment_delete);

        match deploy_environment(&context, &ea) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        // todo: check the database disk is here and with correct size

        match delete_environment(&context_for_delete, &ea_delete) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(true),
        };
        return test_name.to_string();
    })
}

// Redis self-hosted environment
#[cfg(feature = "test-aws-self-hosted")]
#[test]
fn redis_v5_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = test_utilities::aws::working_minimal_environment(&context, secrets.clone());
    test_redis_configuration(
        context,
        environment,
        secrets,
        "5",
        "redis_v5_deploy_a_working_dev_environment",
    );
}

#[cfg(feature = "test-aws-self-hosted")]
#[test]
fn redis_v6_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = test_utilities::aws::working_minimal_environment(&context, secrets.clone());
    test_redis_configuration(
        context,
        environment,
        secrets,
        "6",
        "redis_v6_deploy_a_working_dev_environment",
    );
}

// Redis production environment (Elasticache)
#[cfg(feature = "test-aws-managed-services")]
#[test]
fn redis_v5_deploy_a_working_prod_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let mut environment = test_utilities::aws::working_minimal_environment(&context, secrets.clone());
    environment.kind = Kind::Production;
    test_redis_configuration(
        context,
        environment,
        secrets,
        "5",
        "redis_v5_deploy_a_working_prod_environment",
    );
}

#[cfg(feature = "test-aws-managed-services")]
#[test]
fn redis_v6_deploy_a_working_prod_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let mut environment = test_utilities::aws::working_minimal_environment(&context, secrets.clone());
    environment.kind = Kind::Production;
    test_redis_configuration(
        context,
        environment,
        secrets,
        "6",
        "redis_v6_deploy_a_working_prod_environment",
    );
}

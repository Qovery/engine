use ::function_name::named;
use tracing::{span, Level};

use qovery_engine::models::{
    Action, Application, Clone2, Context, Database, DatabaseKind, Environment, EnvironmentAction, EnvironmentVariable,
    Kind,
};
use qovery_engine::transaction::TransactionResult;
use test_utilities::utilities::{context, engine_run_test, generate_id, init, FuncTestsSecrets};

use crate::scaleway::scw_environment::{delete_environment, deploy_environment};
use test_utilities::scaleway::working_minimal_environment;

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
        init();

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
            database_instance_type: "not-used".to_string(),
            database_disk_type: "scw-sbv-ssd-0".to_string(),
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

        match deploy_environment(&context, ea) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        // todo: check the database disk is here and with correct size

        match delete_environment(&context_for_delete, ea_delete) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(true),
        };
        return test_name.to_string();
    })
}

// Postgres environment environment
#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn postgresql_v10_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(&context, secrets.clone());
    test_postgresql_configuration(context, environment, secrets, "10", function_name!());
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn postgresql_v11_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(&context, secrets.clone());
    test_postgresql_configuration(context, environment, secrets, "11", function_name!());
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn postgresql_v12_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(&context, secrets.clone());
    test_postgresql_configuration(context, environment, secrets, "12", function_name!());
}

// Postgres production environment

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
    engine_run_test(|| {
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
            database_instance_type: "not-used".to_string(),
            database_disk_type: "scw-sbv-ssd-0".to_string(),
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
            .collect::<Vec<Application>>();

        environment.routers[0].routes[0].application_name = app_name.clone();

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;
        let env_action = EnvironmentAction::Environment(environment);
        let env_action_delete = EnvironmentAction::Environment(environment_delete);

        match deploy_environment(&context, env_action) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        // todo: check the database disk is here and with correct size

        match delete_environment(&context_for_delete, env_action_delete) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(true),
        };

        return test_name.to_string();
    })
}

// development environment
#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn mongodb_v3_6_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(&context, secrets.clone());
    test_mongodb_configuration(context, environment, secrets, "3.6", function_name!());
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn mongodb_v4_0_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(&context, secrets.clone());
    test_mongodb_configuration(context, environment, secrets, "4.0", function_name!());
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn mongodb_v4_2_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(&context, secrets.clone());
    test_mongodb_configuration(context, environment, secrets, "4.2", function_name!());
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn mongodb_v4_4_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(&context, secrets.clone());
    test_mongodb_configuration(context, environment, secrets, "4.4", function_name!());
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
        init();

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
            database_instance_type: "not-used".to_string(),
            database_disk_type: "scw-sbv-ssd-0".to_string(),
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

        match deploy_environment(&context, ea) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        // todo: check the database disk is here and with correct size

        match delete_environment(&deletion_context, ea_delete) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        return test_name.to_string();
    })
}

// MySQL self-hosted environment
#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn mysql_v5_7_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(&context, secrets.clone());
    test_mysql_configuration(context, environment, secrets, "5.7", function_name!());
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn mysql_v8_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(&context, secrets.clone());
    test_mysql_configuration(context, environment, secrets, "8.0", function_name!());
}

// MySQL production environment

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
        init();

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
            database_instance_type: "not-used".to_string(),
            database_disk_type: "scw-sbv-ssd-0".to_string(),
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

        match deploy_environment(&context, ea) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        // todo: check the database disk is here and with correct size

        match delete_environment(&context_for_delete, ea_delete) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(true),
        };

        return test_name.to_string();
    })
}

// Redis self-hosted environment
#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn redis_v5_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(&context, secrets.clone());
    test_redis_configuration(context, environment, secrets, "5", function_name!());
}

#[cfg(feature = "test-scw-self-hosted")]
#[named]
#[test]
fn redis_v6_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(&context, secrets.clone());
    test_redis_configuration(context, environment, secrets, "6", function_name!());
}

use ::function_name::named;
use tracing::{span, warn, Level};

use qovery_engine::cloud_provider::{Kind as ProviderKind, Kind};
use qovery_engine::models::{
    Action, Clone2, Context, Database, DatabaseKind, DatabaseMode, Environment, EnvironmentAction, Port, Protocol,
};
use qovery_engine::transaction::TransactionResult;
use test_utilities::utilities::{
    context, engine_run_test, generate_id, get_pods, get_svc_name, init, is_pod_restarted_env, FuncTestsSecrets,
};

use qovery_engine::models::DatabaseMode::{CONTAINER, MANAGED};
use test_utilities::common::{test_db, working_minimal_environment, Infrastructure};
use test_utilities::digitalocean::{
    clean_environments, DO_MANAGED_DATABASE_DISK_TYPE, DO_MANAGED_DATABASE_INSTANCE_TYPE,
    DO_SELF_HOSTED_DATABASE_DISK_TYPE, DO_SELF_HOSTED_DATABASE_INSTANCE_TYPE, DO_TEST_REGION,
};

/**
 **
 ** Global database tests
 **
 **/

// to check overload between several databases and apps
#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
fn deploy_an_environment_with_3_databases_and_3_apps() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let context = context();
        let context_for_deletion = context.clone_not_same_execution_id();
        let secrets = FuncTestsSecrets::new();
        let environment = test_utilities::common::environment_3_apps_3_routers_3_databases(
            &context,
            secrets
                .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
            secrets
                .DEFAULT_TEST_DOMAIN
                .as_ref()
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
            DO_SELF_HOSTED_DATABASE_INSTANCE_TYPE,
            DO_SELF_HOSTED_DATABASE_DISK_TYPE,
            Kind::Do,
        );

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;
        let env_action = EnvironmentAction::Environment(environment.clone());
        let env_action_delete = EnvironmentAction::Environment(environment_delete.clone());

        match environment.deploy_environment(Kind::Do, &context, &env_action) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        match environment_delete.delete_environment(Kind::Do, &context_for_deletion, &env_action_delete) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        // delete images created during test from registries
        if let Err(e) = clean_environments(&context, vec![environment], secrets, DO_TEST_REGION) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        return test_name.to_string();
    })
}

#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
fn deploy_an_environment_with_db_and_pause_it() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let context = context();
        let context_for_deletion = context.clone_not_same_execution_id();
        let secrets = FuncTestsSecrets::new();
        let environment = test_utilities::common::environnement_2_app_2_routers_1_psql(
            &context,
            secrets
                .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
            secrets
                .clone()
                .DEFAULT_TEST_DOMAIN
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
            DO_SELF_HOSTED_DATABASE_INSTANCE_TYPE,
            DO_SELF_HOSTED_DATABASE_DISK_TYPE,
            Kind::Do,
        );

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;
        let env_action = EnvironmentAction::Environment(environment.clone());
        let env_action_delete = EnvironmentAction::Environment(environment_delete.clone());

        match environment.deploy_environment(Kind::Do, &context, &env_action.clone()) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        match environment.pause_environment(Kind::Do, &context, &env_action) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        // Check that we have actually 0 pods running for this db
        let app_name = format!("postgresql{}-0", environment.databases[0].name);
        let ret = get_pods(
            ProviderKind::Do,
            environment.clone(),
            app_name.clone().as_str(),
            secrets
                .DIGITAL_OCEAN_TEST_CLUSTER_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_CLUSTER_ID is not set"),
            secrets.clone(),
        );
        assert_eq!(ret.is_ok(), true);
        assert_eq!(ret.unwrap().items.is_empty(), true);

        match environment_delete.delete_environment(Kind::Do, &context_for_deletion, &env_action_delete) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        // delete images created during test from registries
        if let Err(e) = clean_environments(&context, vec![environment], secrets.clone(), DO_TEST_REGION) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        return test_name.to_string();
    })
}

// this test ensure containers databases are never restarted, even in failover environment case
#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
fn postgresql_failover_dev_environment_with_all_options() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let context = context();
        let context_for_deletion = context.clone_not_same_execution_id();
        let secrets = FuncTestsSecrets::new();
        let test_domain = secrets
            .clone()
            .DEFAULT_TEST_DOMAIN
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets");

        let environment = test_utilities::common::environnement_2_app_2_routers_1_psql(
            &context,
            secrets
                .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
            test_domain.as_str(),
            DO_SELF_HOSTED_DATABASE_INSTANCE_TYPE,
            DO_SELF_HOSTED_DATABASE_DISK_TYPE,
            Kind::Do,
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
            secrets
                .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
            test_domain.as_str(),
            DO_SELF_HOSTED_DATABASE_INSTANCE_TYPE,
            DO_SELF_HOSTED_DATABASE_DISK_TYPE,
            Kind::Do,
        );

        environment_delete.action = Action::Delete;

        let env_action = EnvironmentAction::Environment(environment.clone());
        let env_action_fail_ok =
            EnvironmentAction::EnvironmentWithFailover(environment_never_up.clone(), environment.clone());
        let env_action_for_deletion = EnvironmentAction::Environment(environment_delete.clone());

        match environment.deploy_environment(Kind::Do, &context, &env_action) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };
        // TO CHECK: DATABASE SHOULDN'T BE RESTARTED AFTER A REDEPLOY
        let database_name = format!("postgresql-{}-0", &environment_check.databases[0].name);
        match is_pod_restarted_env(
            ProviderKind::Do,
            secrets
                .DIGITAL_OCEAN_TEST_CLUSTER_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_CLUSTER_ID is not set"),
            environment_check.clone(),
            database_name.as_str(),
            secrets.clone(),
        ) {
            (true, _) => assert!(true),
            (false, _) => assert!(false),
        }
        match environment_never_up.deploy_environment(Kind::Do, &context, &env_action_fail_ok) {
            TransactionResult::Ok => assert!(false),
            TransactionResult::Rollback(_) => assert!(true),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };
        // TO CHECK: DATABASE SHOULDN'T BE RESTARTED AFTER A REDEPLOY EVEN IF FAIL
        match is_pod_restarted_env(
            ProviderKind::Do,
            secrets
                .DIGITAL_OCEAN_TEST_CLUSTER_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_CLUSTER_ID is not set"),
            environment_check.clone(),
            database_name.as_str(),
            secrets.clone(),
        ) {
            (true, _) => assert!(true),
            (false, _) => assert!(false),
        }

        match environment_delete.delete_environment(Kind::Do, &context_for_deletion, &env_action_for_deletion) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        // delete images created during test from registries
        if let Err(e) = clean_environments(&context, vec![environment, environment_delete], secrets, DO_TEST_REGION) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        return test_name.to_string();
    })
}

// Ensure a full environment can run correctly
#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
fn postgresql_deploy_a_working_development_environment_with_all_options() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let context = context();
        let secrets = FuncTestsSecrets::new();
        let context_for_deletion = context.clone_not_same_execution_id();
        let test_domain = secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets");

        let environment = test_utilities::common::environnement_2_app_2_routers_1_psql(
            &context,
            secrets
                .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
            test_domain.as_str(),
            DO_SELF_HOSTED_DATABASE_INSTANCE_TYPE,
            DO_SELF_HOSTED_DATABASE_DISK_TYPE,
            Kind::Do,
        );
        //let env_to_check = environment.clone();
        let mut environment_delete = test_utilities::common::environnement_2_app_2_routers_1_psql(
            &context_for_deletion,
            secrets
                .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
            test_domain.as_str(),
            DO_SELF_HOSTED_DATABASE_INSTANCE_TYPE,
            DO_SELF_HOSTED_DATABASE_DISK_TYPE,
            Kind::Do,
        );

        environment_delete.action = Action::Delete;

        let env_action = EnvironmentAction::Environment(environment.clone());
        let env_action_for_deletion = EnvironmentAction::Environment(environment_delete.clone());

        match environment.deploy_environment(Kind::Do, &context, &env_action) {
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

        match environment_delete.delete_environment(Kind::Do, &context_for_deletion, &env_action_for_deletion) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        // delete images created during test from registries
        if let Err(e) = clean_environments(
            &context,
            vec![environment, environment_delete],
            secrets.clone(),
            DO_TEST_REGION,
        ) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        return test_name.to_string();
    })
}

// Ensure redeploy works as expected
#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
fn postgresql_deploy_a_working_environment_and_redeploy() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let secrets = FuncTestsSecrets::new();
        let context = context();
        let context_for_redeploy = context.clone_not_same_execution_id();
        let context_for_delete = context.clone_not_same_execution_id();

        let mut environment = test_utilities::common::working_minimal_environment(
            &context,
            secrets
                .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
            secrets
                .clone()
                .DEFAULT_TEST_DOMAIN
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );
        let database_mode = CONTAINER;

        let app_name = format!("postgresql-app-{}", generate_id());
        let database_host = get_svc_name(DatabaseKind::Postgresql, Kind::Do).to_string();
        let database_port = 5432;
        let database_db_name = "postgres".to_string();
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
            mode: database_mode.clone(),
            database_instance_type: if database_mode == MANAGED {
                DO_MANAGED_DATABASE_INSTANCE_TYPE
            } else {
                DO_SELF_HOSTED_DATABASE_INSTANCE_TYPE
            }
            .to_string(),
            database_disk_type: if database_mode == MANAGED {
                DO_MANAGED_DATABASE_DISK_TYPE
            } else {
                DO_SELF_HOSTED_DATABASE_DISK_TYPE
            }
            .to_string(),
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
            .collect::<Vec<qovery_engine::models::Application>>();
        environment.routers[0].routes[0].application_name = app_name;

        let environment_to_redeploy = environment.clone();
        let environment_check = environment.clone();
        let env_action_redeploy = EnvironmentAction::Environment(environment_to_redeploy.clone());

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;
        let env_action = EnvironmentAction::Environment(environment.clone());
        let env_action_delete = EnvironmentAction::Environment(environment_delete.clone());

        match environment.deploy_environment(Kind::Do, &context, &env_action) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };
        match environment_to_redeploy.deploy_environment(Kind::Do, &context_for_redeploy, &env_action_redeploy) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };
        // TO CHECK: DATABASE SHOULDN'T BE RESTARTED AFTER A REDEPLOY
        let database_name = format!("postgresql-{}-0", &environment_check.databases[0].name);
        match is_pod_restarted_env(
            ProviderKind::Do,
            secrets
                .DIGITAL_OCEAN_TEST_CLUSTER_ID
                .as_ref()
                .expect("DIGITAL_OCEAN_TEST_CLUSTER_ID is not set"),
            environment_check,
            database_name.as_str(),
            secrets.clone(),
        ) {
            (true, _) => assert!(true),
            (false, _) => assert!(false),
        }

        match environment_delete.delete_environment(Kind::Do, &context_for_delete, &env_action_delete) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(true),
        };

        // delete images created during test from registries
        if let Err(e) = clean_environments(&context, vec![environment], secrets, DO_TEST_REGION) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        return test_name.to_string();
    })
}

/**
 **
 ** PostgreSQL tests
 **
 **/
#[allow(dead_code)]
fn test_postgresql_configuration(
    context: Context,
    environment: Environment,
    secrets: FuncTestsSecrets,
    version: &str,
    test_name: &str,
    database_mode: DatabaseMode,
    is_public: bool,
) {
    engine_run_test(|| {
        test_db(
            context,
            environment,
            secrets,
            version,
            test_name,
            DatabaseKind::Postgresql,
            Kind::Do,
            database_mode,
            is_public,
        )
    })
}

// Postgres environment environment
#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
fn private_postgresql_v10_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(
        &context,
        secrets
            .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str(),
    );
    test_postgresql_configuration(context, environment, secrets, "10", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-do-self-hosted")]
#[ignore]
#[named]
#[test]
fn public_postgresql_v10_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(
        &context,
        secrets
            .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str(),
    );
    test_postgresql_configuration(context, environment, secrets, "10", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-do-self-hosted")]
#[ignore]
#[named]
#[test]
fn private_postgresql_v11_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(
        &context,
        secrets
            .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str(),
    );
    test_postgresql_configuration(context, environment, secrets, "11", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-do-self-hosted")]
#[ignore]
#[named]
#[test]
fn public_postgresql_v11_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(
        &context,
        secrets
            .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str(),
    );
    test_postgresql_configuration(context, environment, secrets, "11", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
fn private_postgresql_v12_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(
        &context,
        secrets
            .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str(),
    );
    test_postgresql_configuration(context, environment, secrets, "12", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
fn public_postgresql_v12_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(
        &context,
        secrets
            .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str(),
    );
    test_postgresql_configuration(context, environment, secrets, "12", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
fn private_postgresql_v13_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(
        &context,
        DO_QOVERY_ORGANIZATION_ID,
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str(),
    );
    test_postgresql_configuration(context, environment, secrets, "13", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
fn public_postgresql_v13_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(
        &context,
        DO_QOVERY_ORGANIZATION_ID,
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str(),
    );
    test_postgresql_configuration(context, environment, secrets, "13", function_name!(), CONTAINER, true);
}

/**
 **
 ** MongoDB tests
 **
 **/
#[allow(dead_code)]
fn test_mongodb_configuration(
    context: Context,
    environment: Environment,
    secrets: FuncTestsSecrets,
    version: &str,
    test_name: &str,
    database_mode: DatabaseMode,
    is_public: bool,
) {
    engine_run_test(|| {
        test_db(
            context,
            environment,
            secrets,
            version,
            test_name,
            DatabaseKind::Mongodb,
            Kind::Do,
            database_mode,
            is_public,
        )
    })
}

// development environment
#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
fn private_mongodb_v3_6_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(
        &context,
        secrets
            .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str(),
    );
    test_mongodb_configuration(context, environment, secrets, "3.6", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-do-self-hosted")]
#[ignore]
#[named]
#[test]
fn public_mongodb_v3_6_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(
        &context,
        secrets
            .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str(),
    );
    test_mongodb_configuration(context, environment, secrets, "3.6", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-do-self-hosted")]
#[ignore]
#[named]
#[test]
fn private_mongodb_v4_0_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(
        &context,
        secrets
            .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str(),
    );
    test_mongodb_configuration(context, environment, secrets, "4.0", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-do-self-hosted")]
#[ignore]
#[named]
#[test]
fn public_mongodb_v4_0_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(
        &context,
        secrets
            .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str(),
    );
    test_mongodb_configuration(context, environment, secrets, "4.0", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-do-self-hosted")]
#[ignore]
#[named]
#[test]
fn private_mongodb_v4_2_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(
        &context,
        secrets
            .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str(),
    );
    test_mongodb_configuration(context, environment, secrets, "4.2", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
fn public_mongodb_v4_2_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(
        &context,
        secrets
            .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str(),
    );
    test_mongodb_configuration(context, environment, secrets, "4.2", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
fn private_mongodb_v4_4_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(
        &context,
        secrets
            .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str(),
    );
    test_mongodb_configuration(context, environment, secrets, "4.4", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
fn public_mongodb_v4_4_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(
        &context,
        secrets
            .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str(),
    );
    test_mongodb_configuration(context, environment, secrets, "4.4", function_name!(), CONTAINER, true);
}

/**
 **
 ** MySQL tests
 **
 **/
#[allow(dead_code)]
fn test_mysql_configuration(
    context: Context,
    environment: Environment,
    secrets: FuncTestsSecrets,
    version: &str,
    test_name: &str,
    database_mode: DatabaseMode,
    is_public: bool,
) {
    engine_run_test(|| {
        test_db(
            context,
            environment,
            secrets,
            version,
            test_name,
            DatabaseKind::Mysql,
            Kind::Do,
            database_mode,
            is_public,
        )
    })
}

// MySQL self-hosted environment
#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
fn private_mysql_v5_7_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(
        &context,
        secrets
            .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str(),
    );
    test_mysql_configuration(context, environment, secrets, "5.7", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-do-self-hosted")]
#[ignore]
#[named]
#[test]
fn public_mysql_v5_7_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(
        &context,
        secrets
            .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str(),
    );
    test_mysql_configuration(context, environment, secrets, "5.7", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-do-self-hosted")]
#[ignore]
#[named]
#[test]
fn private_mysql_v8_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(
        &context,
        secrets
            .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str(),
    );
    test_mysql_configuration(context, environment, secrets, "8.0", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
fn public_mysql_v8_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(
        &context,
        secrets
            .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str(),
    );
    test_mysql_configuration(context, environment, secrets, "8.0", function_name!(), CONTAINER, true);
}

// MySQL production environment

/**
 **
 ** Redis tests
 **
 **/
#[allow(dead_code)]
fn test_redis_configuration(
    context: Context,
    environment: Environment,
    secrets: FuncTestsSecrets,
    version: &str,
    test_name: &str,
    database_mode: DatabaseMode,
    is_public: bool,
) {
    engine_run_test(|| {
        test_db(
            context,
            environment,
            secrets,
            version,
            test_name,
            DatabaseKind::Redis,
            Kind::Do,
            database_mode,
            is_public,
        )
    })
}

// Redis self-hosted environment
#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
fn private_redis_v5_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(
        &context,
        secrets
            .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str(),
    );
    test_redis_configuration(context, environment, secrets, "5", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-do-self-hosted")]
#[ignore]
#[named]
#[test]
fn public_redis_v5_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(
        &context,
        secrets
            .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str(),
    );
    test_redis_configuration(context, environment, secrets, "5", function_name!(), CONTAINER, true);
}

#[cfg(feature = "test-do-self-hosted")]
#[ignore]
#[named]
#[test]
fn private_redis_v6_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(
        &context,
        secrets
            .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str(),
    );
    test_redis_configuration(context, environment, secrets, "6", function_name!(), CONTAINER, false);
}

#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
fn public_redis_v6_deploy_a_working_dev_environment() {
    let context = context();
    let secrets = FuncTestsSecrets::new();
    let environment = working_minimal_environment(
        &context,
        secrets
            .DIGITAL_OCEAN_TEST_ORGANIZATION_ID
            .as_ref()
            .expect("DIGITAL_OCEAN_TEST_ORGANIZATION_ID is not set"),
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str(),
    );
    test_redis_configuration(context, environment, secrets, "6", function_name!(), CONTAINER, true);
}

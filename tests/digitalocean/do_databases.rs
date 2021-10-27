use ::function_name::named;
use tracing::{span, warn, Level};

use qovery_engine::cloud_provider::Kind as ProviderKind;
use qovery_engine::models::{
    Action, Application, Clone2, Context, Database, DatabaseKind, DatabaseMode, Environment, EnvironmentAction,
};
use qovery_engine::transaction::TransactionResult;
use test_utilities::utilities::{
    context, engine_run_test, generate_id, get_pods, get_pvc, get_svc, init, is_pod_restarted_env, FuncTestsSecrets,
};

use qovery_engine::cloud_provider::digitalocean::DO;
use qovery_engine::cmd::structs::SVCItem;
use qovery_engine::models::DatabaseMode::{CONTAINER, MANAGED};
use test_utilities::common::working_minimal_environment;
use test_utilities::digitalocean::{
    clean_environments, delete_environment, deploy_environment, pause_environment, DO_KUBE_TEST_CLUSTER_ID,
    DO_MANAGED_DATABASE_DISK_TYPE, DO_MANAGED_DATABASE_INSTANCE_TYPE, DO_QOVERY_ORGANIZATION_ID,
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
            DO_QOVERY_ORGANIZATION_ID,
            secrets
                .clone()
                .DEFAULT_TEST_DOMAIN
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
            DO_SELF_HOSTED_DATABASE_INSTANCE_TYPE,
            DO_SELF_HOSTED_DATABASE_DISK_TYPE,
        );

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;
        let env_action = EnvironmentAction::Environment(environment.clone());
        let env_action_delete = EnvironmentAction::Environment(environment_delete);

        match deploy_environment(&context, env_action, DO_TEST_REGION) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        match delete_environment(&context_for_deletion, env_action_delete, DO_TEST_REGION) {
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
            DO_QOVERY_ORGANIZATION_ID,
            secrets
                .clone()
                .DEFAULT_TEST_DOMAIN
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
            DO_SELF_HOSTED_DATABASE_INSTANCE_TYPE,
            DO_SELF_HOSTED_DATABASE_DISK_TYPE,
        );

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;
        let env_action = EnvironmentAction::Environment(environment.clone());
        let env_action_delete = EnvironmentAction::Environment(environment_delete);

        match deploy_environment(&context, env_action.clone(), DO_TEST_REGION) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        match pause_environment(&context, env_action, DO_TEST_REGION) {
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
            DO_KUBE_TEST_CLUSTER_ID,
            secrets.clone(),
        );
        assert_eq!(ret.is_ok(), true);
        assert_eq!(ret.unwrap().items.is_empty(), true);

        match delete_environment(&context_for_deletion, env_action_delete, DO_TEST_REGION) {
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
            DO_QOVERY_ORGANIZATION_ID,
            test_domain.as_str(),
            DO_SELF_HOSTED_DATABASE_INSTANCE_TYPE,
            DO_SELF_HOSTED_DATABASE_DISK_TYPE,
        );
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
        let mut environment_delete = test_utilities::common::environnement_2_app_2_routers_1_psql(
            &context_for_deletion,
            DO_QOVERY_ORGANIZATION_ID,
            test_domain.as_str(),
            DO_SELF_HOSTED_DATABASE_INSTANCE_TYPE,
            DO_SELF_HOSTED_DATABASE_DISK_TYPE,
        );

        environment_delete.action = Action::Delete;

        let env_action = EnvironmentAction::Environment(environment.clone());
        let env_action_fail_ok = EnvironmentAction::EnvironmentWithFailover(environment_never_up, environment.clone());
        let env_action_for_deletion = EnvironmentAction::Environment(environment_delete.clone());

        match deploy_environment(&context, env_action, DO_TEST_REGION) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };
        // TO CHECK: DATABASE SHOULDN'T BE RESTARTED AFTER A REDEPLOY
        let database_name = format!("postgresql-{}-0", &environment_check.databases[0].name);
        match is_pod_restarted_env(
            ProviderKind::Do,
            DO_KUBE_TEST_CLUSTER_ID,
            environment_check.clone(),
            database_name.as_str(),
            secrets.clone(),
        ) {
            (true, _) => assert!(true),
            (false, _) => assert!(false),
        }
        match deploy_environment(&context, env_action_fail_ok, DO_TEST_REGION) {
            TransactionResult::Ok => assert!(false),
            TransactionResult::Rollback(_) => assert!(true),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };
        // TO CHECK: DATABASE SHOULDN'T BE RESTARTED AFTER A REDEPLOY EVEN IF FAIL
        match is_pod_restarted_env(
            ProviderKind::Do,
            DO_KUBE_TEST_CLUSTER_ID,
            environment_check.clone(),
            database_name.as_str(),
            secrets.clone(),
        ) {
            (true, _) => assert!(true),
            (false, _) => assert!(false),
        }

        match delete_environment(&context_for_deletion, env_action_for_deletion, DO_TEST_REGION) {
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
            DO_QOVERY_ORGANIZATION_ID,
            test_domain.as_str(),
            DO_SELF_HOSTED_DATABASE_INSTANCE_TYPE,
            DO_SELF_HOSTED_DATABASE_DISK_TYPE,
        );
        //let env_to_check = environment.clone();
        let mut environment_delete = test_utilities::common::environnement_2_app_2_routers_1_psql(
            &context_for_deletion,
            DO_QOVERY_ORGANIZATION_ID,
            test_domain.as_str(),
            DO_SELF_HOSTED_DATABASE_INSTANCE_TYPE,
            DO_SELF_HOSTED_DATABASE_DISK_TYPE,
        );

        environment_delete.action = Action::Delete;

        let env_action = EnvironmentAction::Environment(environment.clone());
        let env_action_for_deletion = EnvironmentAction::Environment(environment_delete.clone());

        match deploy_environment(&context, env_action, DO_TEST_REGION) {
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

        match delete_environment(&context_for_deletion, env_action_for_deletion, DO_TEST_REGION) {
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
            DO_QOVERY_ORGANIZATION_ID,
            secrets
                .clone()
                .DEFAULT_TEST_DOMAIN
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

        let database_mode = CONTAINER;

        let app_name = format!("postgresql-app-{}", generate_id());
        let database_host = format!(
            "postgresql-{}.{}",
            generate_id(),
            secrets
                .clone()
                .DEFAULT_TEST_DOMAIN
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
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
                app.private_port = Some(1234);
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
        let env_action_redeploy = EnvironmentAction::Environment(environment_to_redeploy);

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;
        let env_action = EnvironmentAction::Environment(environment.clone());
        let env_action_delete = EnvironmentAction::Environment(environment_delete);

        match deploy_environment(&context, env_action, DO_TEST_REGION) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };
        match deploy_environment(&context_for_redeploy, env_action_redeploy, DO_TEST_REGION) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };
        // TO CHECK: DATABASE SHOULDN'T BE RESTARTED AFTER A REDEPLOY
        let database_name = format!("postgresql-{}-0", &environment_check.databases[0].name);
        match is_pod_restarted_env(
            ProviderKind::Do,
            DO_KUBE_TEST_CLUSTER_ID,
            environment_check,
            database_name.as_str(),
            secrets.clone(),
        ) {
            (true, _) => assert!(true),
            (false, _) => assert!(false),
        }

        match delete_environment(&context_for_delete, env_action_delete, DO_TEST_REGION) {
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

fn test_postgresql_configuration(
    context: Context,
    mut environment: Environment,
    secrets: FuncTestsSecrets,
    version: &str,
    test_name: &str,
    database_mode: DatabaseMode,
    is_public: bool,
) {
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();
        let context_for_delete = context.clone_not_same_execution_id();

        let app_id = generate_id();
        let app_name = format!("postgresql-app-{}", generate_id());
        let database_host = match is_public {
            true => format!(
                "postgresql-{}.{}",
                generate_id(),
                secrets.DEFAULT_TEST_DOMAIN.as_ref().unwrap()
            ),
            false => match database_mode {
                CONTAINER => format!(
                    "postgresqlpostgres.{}-{}.svc.cluster.local",
                    environment.project_id, environment.id
                ),
                MANAGED => format!(
                    "{}-dns.{}-{}.svc.cluster.local",
                    app_id.clone(),
                    environment.project_id,
                    environment.id
                ),
            },
        };
        let database_port = 5432;
        let database_db_name = "postgres".to_string();
        let database_username = "superuser".to_string();
        let database_password = generate_id();
        let storage_size = 10;

        environment.databases = vec![Database {
            kind: DatabaseKind::Postgresql,
            action: Action::Create,
            id: app_id.clone(),
            name: database_db_name.clone(),
            version: version.to_string(),
            fqdn_id: "postgresql-".to_string() + generate_id().as_str(),
            fqdn: database_host.clone(),
            port: database_port.clone(),
            username: database_username.clone(),
            password: database_password.clone(),
            total_cpus: "100m".to_string(),
            total_ram_in_mib: 512,
            disk_size_in_gib: storage_size.clone(),
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
            publicly_accessible: is_public.clone(),
        }];
        environment.applications = environment
            .applications
            .into_iter()
            .map(|mut app| {
                app.branch = app_name.clone();
                app.commit_id = "ad65b24a0470e7e8aa0983e036fb9a05928fd973".to_string();
                app.private_port = Some(1234);
                app.dockerfile_path = Some(format!("Dockerfile-{}", version));
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
        environment.routers[0].routes[0].application_name = app_name.clone();

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;
        let ea = EnvironmentAction::Environment(environment.clone());
        let ea_delete = EnvironmentAction::Environment(environment_delete);

        match deploy_environment(&context, ea, DO_TEST_REGION) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        match database_mode.clone() {
            CONTAINER => {
                match get_pvc(
                    ProviderKind::Do,
                    DO_KUBE_TEST_CLUSTER_ID,
                    environment.clone(),
                    secrets.clone(),
                ) {
                    Ok(pvc) => assert_eq!(
                        pvc.items.expect("No items in pvc")[0].spec.resources.requests.storage,
                        format!("{}Gi", storage_size)
                    ),
                    Err(_) => assert!(false),
                };

                match get_svc(
                    ProviderKind::Do,
                    DO_KUBE_TEST_CLUSTER_ID,
                    environment.clone(),
                    secrets.clone(),
                ) {
                    Ok(svc) => assert_eq!(
                        svc.items
                            .expect("No items in svc")
                            .into_iter()
                            .filter(|svc| svc.metadata.name.contains("postgresqlpostgres")
                                && &svc.spec.svc_type == "LoadBalancer")
                            .collect::<Vec<SVCItem>>()
                            .len(),
                        match is_public {
                            true => 1,
                            false => 0,
                        }
                    ),
                    Err(_) => assert!(false),
                };
            }
            MANAGED => {
                match get_svc(
                    ProviderKind::Do,
                    DO_KUBE_TEST_CLUSTER_ID,
                    environment.clone(),
                    secrets.clone(),
                ) {
                    Ok(svc) => {
                        let service = svc
                            .items
                            .expect("No items in svc")
                            .into_iter()
                            .filter(|svc| {
                                svc.metadata.name.contains(format!("{}-dns", app_id.clone()).as_str())
                                    && svc.spec.svc_type == "ExternalName"
                            })
                            .collect::<Vec<SVCItem>>();
                        let annotations = &service[0].metadata.annotations;
                        assert_eq!(service.len(), 1);
                        match is_public {
                            true => {
                                assert!(annotations.contains_key("external-dns.alpha.kubernetes.io/hostname"));
                                assert_eq!(annotations["external-dns.alpha.kubernetes.io/hostname"], database_host);
                            }
                            false => assert!(!annotations.contains_key("external-dns.alpha.kubernetes.io/hostname")),
                        }
                    }
                    Err(_) => assert!(false),
                };
            }
        }

        match delete_environment(&context_for_delete, ea_delete, DO_TEST_REGION) {
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

// Postgres environment environment
#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
fn private_postgresql_v10_deploy_a_working_dev_environment() {
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
        DO_QOVERY_ORGANIZATION_ID,
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
        DO_QOVERY_ORGANIZATION_ID,
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
        DO_QOVERY_ORGANIZATION_ID,
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
        DO_QOVERY_ORGANIZATION_ID,
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
        DO_QOVERY_ORGANIZATION_ID,
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str(),
    );
    test_postgresql_configuration(context, environment, secrets, "12", function_name!(), CONTAINER, true);
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
    database_mode: DatabaseMode,
    is_public: bool,
) {
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();
        let context_for_delete = context.clone_not_same_execution_id();

        let app_id = generate_id();
        let app_name = format!("mongodb-app-{}", generate_id());
        let database_host = match is_public {
            true => format!(
                "mongodb-{}.{}",
                generate_id(),
                secrets.DEFAULT_TEST_DOMAIN.as_ref().unwrap()
            ),
            false => match database_mode {
                CONTAINER => format!(
                    "mongodbmymongodb.{}-{}.svc.cluster.local",
                    environment.project_id, environment.id
                ),
                MANAGED => format!(
                    "{}-dns.{}-{}.svc.cluster.local",
                    app_id.clone(),
                    environment.project_id,
                    environment.id
                ),
            },
        };
        let database_port = 27017;
        let database_db_name = "my-mongodb".to_string();
        let database_username = "superuser".to_string();
        let database_password = generate_id();
        let database_uri = format!(
            "mongodb://{}:{}@{}:{}/{}",
            database_username, database_password, database_host, database_port, database_db_name
        );
        let storage_size = 10;

        environment.databases = vec![Database {
            kind: DatabaseKind::Mongodb,
            action: Action::Create,
            id: app_id.clone(),
            name: database_db_name.clone(),
            version: version.to_string(),
            fqdn_id: "mongodb-".to_string() + generate_id().as_str(),
            fqdn: database_host.clone(),
            port: database_port.clone(),
            username: database_username.clone(),
            password: database_password.clone(),
            total_cpus: "500m".to_string(),
            total_ram_in_mib: 512,
            disk_size_in_gib: storage_size.clone(),
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
            publicly_accessible: is_public.clone(),
        }];

        environment.applications = environment
            .applications
            .into_iter()
            .map(|mut app| {
                app.branch = app_name.clone();
                app.commit_id = "3fdc7e784c1d98b80446be7ff25e35370306d9a8".to_string();
                app.private_port = Some(1234);
                app.dockerfile_path = Some(format!("Dockerfile-{}", version));
                app.environment_vars = btreemap! {
                    "QOVERY_DATABASE_TESTING_DATABASE_FQDN".to_string() => base64::encode(database_host.clone()),
                    "QOVERY_DATABASE_MY_DDB_CONNECTION_URI".to_string() => base64::encode(database_uri.clone()),
                    "QOVERY_DATABASE_TESTING_DATABASE_PORT".to_string() => base64::encode(database_port.to_string()),
                    "MONGODB_DBNAME".to_string() => base64::encode(database_db_name.clone()),
                    "QOVERY_DATABASE_TESTING_DATABASE_USERNAME".to_string() => base64::encode(database_username.clone()),
                    "QOVERY_DATABASE_TESTING_DATABASE_PASSWORD".to_string() => base64::encode(database_password.clone()),
                };
                app
            })
            .collect::<Vec<Application>>();

        environment.routers[0].routes[0].application_name = app_name.clone();

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;
        let env_action = EnvironmentAction::Environment(environment.clone());
        let env_action_delete = EnvironmentAction::Environment(environment_delete);

        match deploy_environment(&context, env_action, DO_TEST_REGION) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        match database_mode.clone() {
            CONTAINER => {
                match get_pvc(
                    ProviderKind::Do,
                    DO_KUBE_TEST_CLUSTER_ID,
                    environment.clone(),
                    secrets.clone(),
                ) {
                    Ok(pvc) => {
                        assert_eq!(
                            pvc.items.expect("No items in pvc")[0].spec.resources.requests.storage,
                            format!("{}Gi", storage_size)
                        )
                    }
                    Err(_) => assert!(false),
                };

                match get_svc(
                    ProviderKind::Do,
                    DO_KUBE_TEST_CLUSTER_ID,
                    environment.clone(),
                    secrets.clone(),
                ) {
                    Ok(svc) => assert_eq!(
                        svc.items
                            .expect("No items in svc")
                            .into_iter()
                            .filter(|svc| svc.metadata.name.contains("mongodbmymongodb")
                                && &svc.spec.svc_type == "LoadBalancer")
                            .collect::<Vec<SVCItem>>()
                            .len(),
                        match is_public {
                            true => 1,
                            false => 0,
                        }
                    ),
                    Err(_) => assert!(false),
                };
            }
            MANAGED => {
                match get_svc(
                    ProviderKind::Do,
                    DO_KUBE_TEST_CLUSTER_ID,
                    environment.clone(),
                    secrets.clone(),
                ) {
                    Ok(svc) => {
                        let service = svc
                            .items
                            .expect("No items in svc")
                            .into_iter()
                            .filter(|svc| {
                                svc.metadata.name.contains(format!("{}-dns", app_id.clone()).as_str())
                                    && svc.spec.svc_type == "ExternalName"
                            })
                            .collect::<Vec<SVCItem>>();
                        let annotations = &service[0].metadata.annotations;
                        assert_eq!(service.len(), 1);
                        match is_public {
                            true => {
                                assert!(annotations.contains_key("external-dns.alpha.kubernetes.io/hostname"));
                                assert_eq!(annotations["external-dns.alpha.kubernetes.io/hostname"], database_host);
                            }
                            false => assert!(!annotations.contains_key("external-dns.alpha.kubernetes.io/hostname")),
                        }
                    }
                    Err(_) => assert!(false),
                };
            }
        }

        match delete_environment(&context_for_delete, env_action_delete, DO_TEST_REGION) {
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

// development environment
#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
fn private_mongodb_v3_6_deploy_a_working_dev_environment() {
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
        DO_QOVERY_ORGANIZATION_ID,
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
        DO_QOVERY_ORGANIZATION_ID,
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
        DO_QOVERY_ORGANIZATION_ID,
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
        DO_QOVERY_ORGANIZATION_ID,
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
        DO_QOVERY_ORGANIZATION_ID,
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
        DO_QOVERY_ORGANIZATION_ID,
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
        DO_QOVERY_ORGANIZATION_ID,
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

fn test_mysql_configuration(
    context: Context,
    mut environment: Environment,
    secrets: FuncTestsSecrets,
    version: &str,
    test_name: &str,
    database_mode: DatabaseMode,
    is_public: bool,
) {
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let deletion_context = context.clone_not_same_execution_id();

        let app_id = generate_id();
        let app_name = format!("mysql-app-{}", generate_id());
        let database_host = match is_public {
            true => format!(
                "mysql-{}.{}",
                generate_id(),
                secrets.DEFAULT_TEST_DOMAIN.as_ref().unwrap()
            ),
            false => match database_mode {
                CONTAINER => format!(
                    "mysqlmysqldatabase.{}-{}.svc.cluster.local",
                    environment.project_id, environment.id
                ),
                MANAGED => format!(
                    "{}-dns.{}-{}.svc.cluster.local",
                    app_id.clone(),
                    environment.project_id,
                    environment.id
                ),
            },
        };

        let database_port = 3306;
        let database_db_name = "mysqldatabase".to_string();
        let database_username = "superuser".to_string();
        let database_password = generate_id();
        let storage_size = 10;

        environment.databases = vec![Database {
            kind: DatabaseKind::Mysql,
            action: Action::Create,
            id: app_id.clone(),
            name: database_db_name.clone(),
            version: version.to_string(),
            fqdn_id: "mysql-".to_string() + generate_id().as_str(),
            fqdn: database_host.clone(),
            port: database_port.clone(),
            username: database_username.clone(),
            password: database_password.clone(),
            total_cpus: "500m".to_string(),
            total_ram_in_mib: 512,
            disk_size_in_gib: storage_size.clone(),
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
            publicly_accessible: is_public.clone(),
        }];
        environment.applications = environment
            .applications
            .into_iter()
            .map(|mut app| {
                app.branch = app_name.clone();
                app.commit_id = "fc8a87b39cdee84bb789893fb823e3e62a1999c0".to_string();
                app.private_port = Some(1234);
                app.dockerfile_path = Some(format!("Dockerfile-{}", version));
                app.environment_vars = btreemap! {
                    "MYSQL_HOST".to_string() => base64::encode(database_host.clone()),
                    "MYSQL_PORT".to_string() => base64::encode(database_port.to_string()),
                    "MYSQL_DBNAME".to_string()   => base64::encode(database_db_name.clone()),
                    "MYSQL_USERNAME".to_string() => base64::encode(database_username.clone()),
                    "MYSQL_PASSWORD".to_string() => base64::encode(database_password.clone()),
                };
                app
            })
            .collect::<Vec<qovery_engine::models::Application>>();
        environment.routers[0].routes[0].application_name = app_name.clone();

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;
        let ea = EnvironmentAction::Environment(environment.clone());
        let ea_delete = EnvironmentAction::Environment(environment_delete);

        match deploy_environment(&context, ea, DO_TEST_REGION) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        match database_mode.clone() {
            CONTAINER => {
                match get_pvc(
                    ProviderKind::Do,
                    DO_KUBE_TEST_CLUSTER_ID,
                    environment.clone(),
                    secrets.clone(),
                ) {
                    Ok(pvc) => {
                        assert_eq!(
                            pvc.items.expect("No items in pvc")[0].spec.resources.requests.storage,
                            format!("{}Gi", storage_size)
                        )
                    }
                    Err(_) => assert!(false),
                };

                match get_svc(
                    ProviderKind::Do,
                    DO_KUBE_TEST_CLUSTER_ID,
                    environment.clone(),
                    secrets.clone(),
                ) {
                    Ok(svc) => assert_eq!(
                        svc.items
                            .expect("No items in svc")
                            .into_iter()
                            .filter(|svc| svc.metadata.name.contains("mysqlmysqldatabase")
                                && &svc.spec.svc_type == "LoadBalancer")
                            .collect::<Vec<SVCItem>>()
                            .len(),
                        match is_public {
                            true => 1,
                            false => 0,
                        }
                    ),
                    Err(_) => assert!(false),
                };
            }
            MANAGED => {
                match get_svc(
                    ProviderKind::Do,
                    DO_KUBE_TEST_CLUSTER_ID,
                    environment.clone(),
                    secrets.clone(),
                ) {
                    Ok(svc) => {
                        let service = svc
                            .items
                            .expect("No items in svc")
                            .into_iter()
                            .filter(|svc| {
                                svc.metadata.name.contains(format!("{}-dns", app_id.clone()).as_str())
                                    && svc.spec.svc_type == "ExternalName"
                            })
                            .collect::<Vec<SVCItem>>();
                        let annotations = &service[0].metadata.annotations;
                        assert_eq!(service.len(), 1);
                        match is_public {
                            true => {
                                assert!(annotations.contains_key("external-dns.alpha.kubernetes.io/hostname"));
                                assert_eq!(annotations["external-dns.alpha.kubernetes.io/hostname"], database_host);
                            }
                            false => assert!(!annotations.contains_key("external-dns.alpha.kubernetes.io/hostname")),
                        }
                    }
                    Err(_) => assert!(false),
                };
            }
        }

        match delete_environment(&deletion_context, ea_delete, DO_TEST_REGION) {
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

// MySQL self-hosted environment
#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
fn private_mysql_v5_7_deploy_a_working_dev_environment() {
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
        DO_QOVERY_ORGANIZATION_ID,
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
        DO_QOVERY_ORGANIZATION_ID,
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
        DO_QOVERY_ORGANIZATION_ID,
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

fn test_redis_configuration(
    context: Context,
    mut environment: Environment,
    secrets: FuncTestsSecrets,
    version: &str,
    test_name: &str,
    database_mode: DatabaseMode,
    is_public: bool,
) {
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let context_for_delete = context.clone_not_same_execution_id();

        let app_id = generate_id();
        let app_name = format!("redis-app-{}", generate_id());
        let database_host = match is_public {
            true => format!(
                "redis-{}.{}",
                generate_id(),
                secrets.DEFAULT_TEST_DOMAIN.as_ref().unwrap()
            ),
            false => match database_mode {
                CONTAINER => format!(
                    "redismyredis-master.{}-{}.svc.cluster.local",
                    environment.project_id, environment.id
                ),
                MANAGED => format!(
                    "{}-dns.{}-{}.svc.cluster.local",
                    app_id.clone(),
                    environment.project_id,
                    environment.id
                ),
            },
        };
        let database_port = 6379;
        let database_db_name = "my-redis".to_string();
        let database_username = "superuser".to_string();
        let database_password = generate_id();
        let storage_size = 10;

        environment.databases = vec![Database {
            kind: DatabaseKind::Redis,
            action: Action::Create,
            id: app_id.clone(),
            name: database_db_name.clone(),
            version: version.to_string(),
            fqdn_id: "redis-".to_string() + generate_id().as_str(),
            fqdn: database_host.clone(),
            port: database_port.clone(),
            username: database_username.clone(),
            password: database_password.clone(),
            total_cpus: "500m".to_string(),
            total_ram_in_mib: 512,
            disk_size_in_gib: storage_size.clone(),
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
            publicly_accessible: is_public.clone(),
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
                app.environment_vars = btreemap! {
                    "REDIS_HOST".to_string()      => base64::encode(database_host.clone()),
                    "REDIS_PORT".to_string()      => base64::encode(database_port.clone().to_string()),
                    "REDIS_USERNAME".to_string()  => base64::encode(database_username.clone()),
                    "REDIS_PASSWORD".to_string()  => base64::encode(database_password.clone()),
                };
                app
            })
            .collect::<Vec<qovery_engine::models::Application>>();
        environment.routers[0].routes[0].application_name = app_name.clone();

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;
        let ea = EnvironmentAction::Environment(environment.clone());
        let ea_delete = EnvironmentAction::Environment(environment_delete);

        match deploy_environment(&context, ea, DO_TEST_REGION) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        match database_mode.clone() {
            CONTAINER => {
                match get_pvc(
                    ProviderKind::Do,
                    DO_KUBE_TEST_CLUSTER_ID,
                    environment.clone(),
                    secrets.clone(),
                ) {
                    Ok(pvc) => {
                        assert_eq!(
                            pvc.items.expect("No items in pvc")[0].spec.resources.requests.storage,
                            format!("{}Gi", storage_size)
                        )
                    }
                    Err(_) => assert!(false),
                };

                match get_svc(
                    ProviderKind::Do,
                    DO_KUBE_TEST_CLUSTER_ID,
                    environment.clone(),
                    secrets.clone(),
                ) {
                    Ok(svc) => assert_eq!(
                        svc.items
                            .expect("No items in svc")
                            .into_iter()
                            .filter(|svc| svc.metadata.name.contains("redismyredis-master")
                                && &svc.spec.svc_type == "LoadBalancer")
                            .collect::<Vec<SVCItem>>()
                            .len(),
                        match is_public {
                            true => 1,
                            false => 0,
                        }
                    ),
                    Err(_) => assert!(false),
                };
            }
            MANAGED => {
                match get_svc(
                    ProviderKind::Do,
                    DO_KUBE_TEST_CLUSTER_ID,
                    environment.clone(),
                    secrets.clone(),
                ) {
                    Ok(svc) => {
                        let service = svc
                            .items
                            .expect("No items in svc")
                            .into_iter()
                            .filter(|svc| {
                                svc.metadata.name.contains(format!("{}-dns", app_id.clone()).as_str())
                                    && svc.spec.svc_type == "ExternalName"
                            })
                            .collect::<Vec<SVCItem>>();
                        let annotations = &service[0].metadata.annotations;
                        assert_eq!(service.len(), 1);
                        match is_public {
                            true => {
                                assert!(annotations.contains_key("external-dns.alpha.kubernetes.io/hostname"));
                                assert_eq!(annotations["external-dns.alpha.kubernetes.io/hostname"], database_host);
                            }
                            false => assert!(!annotations.contains_key("external-dns.alpha.kubernetes.io/hostname")),
                        }
                    }
                    Err(_) => assert!(false),
                };
            }
        }

        match delete_environment(&context_for_delete, ea_delete, DO_TEST_REGION) {
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

// Redis self-hosted environment
#[cfg(feature = "test-do-self-hosted")]
#[named]
#[test]
fn private_redis_v5_deploy_a_working_dev_environment() {
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
        DO_QOVERY_ORGANIZATION_ID,
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
        DO_QOVERY_ORGANIZATION_ID,
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
        DO_QOVERY_ORGANIZATION_ID,
        secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
            .as_str(),
    );
    test_redis_configuration(context, environment, secrets, "6", function_name!(), CONTAINER, true);
}

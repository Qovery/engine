extern crate test_utilities;

use self::test_utilities::utilities::generate_id;
use qovery_engine::cloud_provider::service::Router;
use qovery_engine::cmd;
use qovery_engine::models::{
    Action, Context, CustomDomain, Database, DatabaseKind, EnvironmentAction, EnvironmentVariable,
    Kind, Storage, StorageType,
};
use qovery_engine::transaction::TransactionResult;
use rusoto_core::region::Region::Custom;
use test_utilities::aws::context;
use test_utilities::utilities::init;

fn deploy_environment(
    context: &Context,
    environment_action: &EnvironmentAction,
) -> TransactionResult {
    let engine = test_utilities::aws::docker_ecr_aws_engine(&context);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let cp = test_utilities::aws::cloud_provider_aws(&context);
    let nodes = test_utilities::aws::aws_kubernetes_nodes();

    let k = test_utilities::aws::aws_kubernetes_eks(&context, &cp, nodes);

    tx.deploy_environment(&k, &environment_action);

    tx.commit()
}

fn pause_environment(
    context: &Context,
    environment_action: &EnvironmentAction,
) -> TransactionResult {
    let engine = test_utilities::aws::docker_ecr_aws_engine(&context);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let cp = test_utilities::aws::cloud_provider_aws(&context);
    let nodes = test_utilities::aws::aws_kubernetes_nodes();

    let k = test_utilities::aws::aws_kubernetes_eks(&context, &cp, nodes);

    tx.pause_environment(&k, &environment_action);

    tx.commit()
}

fn delete_environment(
    context: &Context,
    environment_action: &EnvironmentAction,
) -> TransactionResult {
    let engine = test_utilities::aws::docker_ecr_aws_engine(&context);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let cp = test_utilities::aws::cloud_provider_aws(&context);
    let nodes = test_utilities::aws::aws_kubernetes_nodes();

    let k = test_utilities::aws::aws_kubernetes_eks(&context, &cp, nodes);

    tx.delete_environment(&k, &environment_action);

    tx.commit()
}

#[test]
fn deploy_a_working_environment_with_no_router_on_aws_eks() {
    init();

    let context = context();

    let mut environment = test_utilities::aws::working_minimal_environment(&context);

    environment.routers = vec![];

    let mut environment_delete = environment.clone();
    environment_delete.action = Action::Delete;
    let ea = EnvironmentAction::Environment(environment);
    let ea_delete = EnvironmentAction::Environment(environment_delete);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    match delete_environment(&context, &ea_delete) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    //Todo: remove the namespace (or project)
}

#[test]
fn deploy_a_working_environment_with_domain() {
    init();

    let context = context();

    let mut environment = test_utilities::aws::working_minimal_environment(&context);

    let mut environment_delete = environment.clone();
    environment_delete.action = Action::Delete;
    let ea = EnvironmentAction::Environment(environment);
    let ea_delete = EnvironmentAction::Environment(environment_delete);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    match delete_environment(&context, &ea_delete) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    //Todo: remove the namespace (or project)
}

#[test]
fn deploy_a_working_environment_with_custom_domain() {
    init();

    let context = context();

    let mut environment = test_utilities::aws::working_minimal_environment(&context);
    // Todo: fix domains
    environment.routers = environment
        .routers
        .into_iter()
        .map(|mut router| {
            router.custom_domains = vec![CustomDomain {
                domain: "my-custom.oom.sh".to_string(),
                target_domain: "my-custom.oom.sh".to_string(),
            }];
            router
        })
        .collect::<Vec<qovery_engine::models::Router>>();

    let mut environment_delete = environment.clone();
    environment_delete.action = Action::Delete;
    let ea = EnvironmentAction::Environment(environment);
    let ea_delete = EnvironmentAction::Environment(environment_delete);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    // Todo: check the domain is ready and setup one if needed

    match delete_environment(&context, &ea_delete) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    //Todo: remove the namespace (or project)
}

#[test]
fn deploy_a_working_environment_with_storage_on_aws_eks() {
    init();

    let context = context();

    let mut environment = test_utilities::aws::working_minimal_environment(&context);

    // Todo: make an image that check there is a mounted disk
    environment.applications = environment
        .applications
        .into_iter()
        .map(|mut app| {
            app.storage = vec![Storage {
                id: generate_id(),
                name: "photos".to_string(),
                storage_type: StorageType::Ssd,
                size_in_gib: 10,
                mount_point: "/mnt/photos".to_string(),
                snapshot_retention_in_days: 0,
            }];
            app
        })
        .collect::<Vec<qovery_engine::models::Application>>();

    let mut environment_delete = environment.clone();
    environment_delete.action = Action::Delete;
    let ea = EnvironmentAction::Environment(environment);
    let ea_delete = EnvironmentAction::Environment(environment_delete);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    // todo: check the disk is here and with correct size

    match delete_environment(&context, &ea_delete) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    //Todo: remove the namespace (or project)
}

#[test]
fn deploy_a_working_environment_with_postgresql() {
    init();

    let context = context();

    let mut environment = test_utilities::aws::working_minimal_environment(&context);

    let database_host = "postgresql-".to_string() + generate_id().as_str() + ".oom.sh"; // External access check
    let database_port = 5432;
    let database_db_name = "my-postgres".to_string();
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
        port: database_port.clone(),
        username: database_username.clone(),
        password: database_password.clone(),
        total_cpus: "500m".to_string(),
        total_ram_in_mib: 512,
        disk_size_in_gib: 10,
    }];
    environment.applications = environment
        .applications
        .into_iter()
        .map(|mut app| {
            app.branch = "postgres-app".to_string();
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

    match delete_environment(&context, &ea_delete) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    //Todo: remove the namespace (or project)
}

// Todo: Can't work, missing implementation, MySQL is not bootstraped
#[test]
fn deploy_a_working_environment_with_mysql() {
    init();

    let context = context();

    let mut environment = test_utilities::aws::working_minimal_environment(&context);

    let database_host = "mysql-".to_string() + generate_id().as_str() + ".oom.sh"; // External access check
    let database_port = 3306;
    let database_db_name = "mydb".to_string();
    let database_username = "superuser".to_string();
    let database_password = generate_id();
    environment.databases = vec![Database {
        kind: DatabaseKind::Mysql,
        action: Action::Create,
        id: generate_id(),
        name: database_db_name.clone(),
        version: "5.7.30".to_string(),
        fqdn_id: "mysql-".to_string() + generate_id().as_str(),
        fqdn: database_host.clone(),
        port: database_port.clone(),
        username: database_username.clone(),
        password: database_password.clone(),
        total_cpus: "500m".to_string(),
        total_ram_in_mib: 512,
        disk_size_in_gib: 10,
    }];
    environment.applications = environment
        .applications
        .into_iter()
        .map(|mut app| {
            app.branch = "mysql-app".to_string();
            app.commit_id = "222295112d58d78227c21060d3a707687302e86f".to_string();
            app.private_port = Some(1234);
            app.environment_variables = vec![
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

    match delete_environment(&context, &ea_delete) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    //Todo: remove the namespace (or project)
}

#[test]
fn deploy_a_working_development_environment_with_all_options_on_aws_eks() {
    init();

    let context = context();

    let mut environment = test_utilities::aws::working_environment(&context);
    environment.kind = Kind::Development;

    let ea = EnvironmentAction::Environment(environment);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
fn deploy_a_working_production_environment_with_all_options_on_aws_eks() {
    init();

    let context = context();

    let mut environment = test_utilities::aws::working_environment(&context);
    environment.kind = Kind::Production;

    let ea = EnvironmentAction::Environment(environment);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
fn deploy_a_non_working_environment_with_no_failover_on_aws_eks() {
    init();

    let context = context();

    let mut environment = test_utilities::aws::non_working_environment(&context);

    let ea = EnvironmentAction::Environment(environment);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(false),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(true),
    };
}

#[test]
fn deploy_a_non_working_environment_with_a_working_failover_on_aws_eks() {
    init();

    let context = context();

    let mut environment = test_utilities::aws::non_working_environment(&context);
    let mut failover_environment = test_utilities::aws::working_environment(&context);

    let ea = EnvironmentAction::EnvironmentWithFailover(environment, failover_environment);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(false),
        TransactionResult::Rollback(_) => assert!(true),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
fn deploy_a_non_working_environment_with_a_non_working_failover_on_aws_eks() {
    init();

    let context = context();

    let mut environment = test_utilities::aws::non_working_environment(&context);
    let mut failover_environment = test_utilities::aws::non_working_environment(&context);

    let ea = EnvironmentAction::EnvironmentWithFailover(environment, failover_environment);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(false),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(true),
    };
}

#[test]
fn deploy_a_working_environment_with_a_failing_default_domain_on_aws_eks() {
    init();

    // TODO
}

#[test]
fn deploy_but_fail_to_push_image_on_container_registry() {
    init();

    // TODO
}

#[test]
fn delete_a_working_development_environment_on_aws_eks() {
    init();

    // DEPLOY
    let context = context();

    let mut environment = test_utilities::aws::working_environment(&context);
    environment.kind = Kind::Development;

    let ea = EnvironmentAction::Environment(environment);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    // DELETE
    let context = test_utilities::aws::context();

    let mut environment = test_utilities::aws::working_environment(&context);
    environment.kind = Kind::Development;

    let ea = EnvironmentAction::Environment(environment);

    match delete_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
fn delete_a_working_production_environment_on_aws_eks() {
    init();

    // DEPLOY
    let context = context();

    let mut environment = test_utilities::aws::working_environment(&context);
    environment.kind = Kind::Production;

    let ea = EnvironmentAction::Environment(environment);

    match delete_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    // DELETE
    let context = test_utilities::aws::context();

    let mut environment = test_utilities::aws::working_environment(&context);
    environment.kind = Kind::Production;

    let ea = EnvironmentAction::Environment(environment);

    match delete_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
fn delete_a_non_working_environment_on_aws_eks() {
    init();

    // DEPLOY
    let context = test_utilities::aws::context();

    let mut environment = test_utilities::aws::non_working_environment(&context);
    environment.kind = Kind::Development;

    let ea = EnvironmentAction::Environment(environment);

    match delete_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    // DELETE
    let context = test_utilities::aws::context();

    let mut environment = test_utilities::aws::non_working_environment(&context);
    environment.kind = Kind::Development;

    let ea = EnvironmentAction::Environment(environment);

    match delete_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
fn pause_a_working_development_environment_on_aws_eks() {
    init();

    let context = test_utilities::aws::context();

    let mut environment = test_utilities::aws::working_environment(&context);
    environment.kind = Kind::Development;

    let ea = EnvironmentAction::Environment(environment);

    match pause_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
fn pause_a_working_production_environment_on_aws_eks() {
    init();

    let context = test_utilities::aws::context();

    let mut environment = test_utilities::aws::working_environment(&context);
    environment.kind = Kind::Production;

    let ea = EnvironmentAction::Environment(environment);

    match pause_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
fn pause_a_non_working_environment_on_aws_eks() {
    init();

    let context = test_utilities::aws::context();

    let mut environment = test_utilities::aws::non_working_environment(&context);

    let ea = EnvironmentAction::Environment(environment);

    match pause_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
fn start_and_pause_and_start_and_delete_a_working_environment_on_aws_eks() {
    init();

    // START
    let context = test_utilities::aws::context();

    let mut environment = test_utilities::aws::working_environment(&context);
    let ea = EnvironmentAction::Environment(environment);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    // PAUSE
    let context = test_utilities::aws::context();

    let mut environment = test_utilities::aws::working_environment(&context);
    let ea = EnvironmentAction::Environment(environment);

    match pause_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    // START
    let context = test_utilities::aws::context();

    let mut environment = test_utilities::aws::working_environment(&context);
    let ea = EnvironmentAction::Environment(environment);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    // DELETE
    let context = test_utilities::aws::context();

    let mut environment = test_utilities::aws::working_environment(&context);
    let ea = EnvironmentAction::Environment(environment);

    match delete_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

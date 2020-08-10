extern crate test_utilities;

use qovery_engine::models::{Context, EnvironmentAction, Kind};
use qovery_engine::transaction::TransactionResult;
use test_utilities::context;

fn deploy_environment(
    context: &Context,
    environment_action: &EnvironmentAction,
) -> TransactionResult {
    let engine = test_utilities::docker_ecr_aws_engine(&context);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let cp = test_utilities::cloud_provider_aws(&context);
    let nodes = test_utilities::aws_kubernetes_nodes();

    let k = test_utilities::aws_kubernetes_eks(&context, &cp, nodes);

    tx.deploy_environment(&k, &environment_action);

    tx.commit()
}

fn pause_environment(
    context: &Context,
    environment_action: &EnvironmentAction,
) -> TransactionResult {
    let engine = test_utilities::docker_ecr_aws_engine(&context);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let cp = test_utilities::cloud_provider_aws(&context);
    let nodes = test_utilities::aws_kubernetes_nodes();

    let k = test_utilities::aws_kubernetes_eks(&context, &cp, nodes);

    tx.pause_environment(&k, &environment_action);

    tx.commit()
}

fn delete_environment(
    context: &Context,
    environment_action: &EnvironmentAction,
) -> TransactionResult {
    let engine = test_utilities::docker_ecr_aws_engine(&context);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let cp = test_utilities::cloud_provider_aws(&context);
    let nodes = test_utilities::aws_kubernetes_nodes();

    let k = test_utilities::aws_kubernetes_eks(&context, &cp, nodes);

    tx.delete_environment(&k, &environment_action);

    tx.commit()
}

#[test]
fn deploy_a_working_development_environment_with_all_options_on_aws_eks() {
    test_utilities::init();

    let context = context();

    let mut environment = test_utilities::working_environment(&context);
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
    test_utilities::init();

    let context = context();

    let mut environment = test_utilities::working_environment(&context);
    environment.kind = Kind::Production;

    let ea = EnvironmentAction::Environment(environment);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
fn deploy_a_working_environment_with_no_router_on_aws_eks() {
    test_utilities::init();

    let context = context();

    let mut environment = test_utilities::working_environment(&context);

    environment.routers = vec![];

    let ea = EnvironmentAction::Environment(environment);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
fn deploy_a_working_environment_with_no_database_on_aws_eks() {
    test_utilities::init();

    let context = context();

    let mut environment = test_utilities::working_environment(&context);

    environment.databases = vec![];

    let ea = EnvironmentAction::Environment(environment);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
fn deploy_a_working_environment_with_no_storage_on_aws_eks() {
    test_utilities::init();

    let context = context();

    let mut environment = test_utilities::working_environment(&context);

    environment.applications = environment
        .applications
        .into_iter()
        .map(|mut app| {
            app.storage = vec![];
            app
        })
        .collect::<Vec<_>>();

    let ea = EnvironmentAction::Environment(environment);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
fn deploy_a_working_environment_with_no_custom_domain_on_aws_eks() {
    test_utilities::init();

    let context = context();

    let mut environment = test_utilities::working_environment(&context);

    environment.routers = environment
        .routers
        .into_iter()
        .map(|mut router| {
            router.custom_domains = vec![];
            router
        })
        .collect::<Vec<_>>();

    let ea = EnvironmentAction::Environment(environment);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
fn deploy_a_non_working_environment_with_no_failover_on_aws_eks() {
    test_utilities::init();

    let context = context();

    let mut environment = test_utilities::non_working_environment(&context);

    let ea = EnvironmentAction::Environment(environment);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(false),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(true),
    };
}

#[test]
fn deploy_a_non_working_environment_with_a_working_failover_on_aws_eks() {
    test_utilities::init();

    let context = context();

    let mut environment = test_utilities::non_working_environment(&context);
    let mut failover_environment = test_utilities::working_environment(&context);

    let ea = EnvironmentAction::EnvironmentWithFailover(environment, failover_environment);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(false),
        TransactionResult::Rollback(_) => assert!(true),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
fn deploy_a_non_working_environment_with_a_non_working_failover_on_aws_eks() {
    test_utilities::init();

    let context = context();

    let mut environment = test_utilities::non_working_environment(&context);
    let mut failover_environment = test_utilities::non_working_environment(&context);

    let ea = EnvironmentAction::EnvironmentWithFailover(environment, failover_environment);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(false),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(true),
    };
}

#[test]
fn deploy_a_working_environment_with_a_failing_default_domain_on_aws_eks() {
    test_utilities::init();

    // TODO
}

#[test]
fn deploy_but_fail_to_push_image_on_container_registry() {
    test_utilities::init();

    // TODO
}

#[test]
fn delete_a_working_development_environment_on_aws_eks() {
    test_utilities::init();

    // DEPLOY
    let context = context();

    let mut environment = test_utilities::working_environment(&context);
    environment.kind = Kind::Development;

    let ea = EnvironmentAction::Environment(environment);

    match delete_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    // DELETE
    let context = test_utilities::context();

    let mut environment = test_utilities::working_environment(&context);
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
    test_utilities::init();

    // DEPLOY
    let context = context();

    let mut environment = test_utilities::working_environment(&context);
    environment.kind = Kind::Production;

    let ea = EnvironmentAction::Environment(environment);

    match delete_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    // DELETE
    let context = test_utilities::context();

    let mut environment = test_utilities::working_environment(&context);
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
    test_utilities::init();

    // DEPLOY
    let context = test_utilities::context();

    let mut environment = test_utilities::non_working_environment(&context);
    environment.kind = Kind::Development;

    let ea = EnvironmentAction::Environment(environment);

    match delete_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    // DELETE
    let context = test_utilities::context();

    let mut environment = test_utilities::non_working_environment(&context);
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
    test_utilities::init();

    let context = test_utilities::context();

    let mut environment = test_utilities::working_environment(&context);
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
    test_utilities::init();

    let context = test_utilities::context();

    let mut environment = test_utilities::working_environment(&context);
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
    test_utilities::init();

    let context = test_utilities::context();

    let mut environment = test_utilities::non_working_environment(&context);

    let ea = EnvironmentAction::Environment(environment);

    match pause_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
fn start_and_pause_and_start_and_delete_a_working_environment_on_aws_eks() {
    test_utilities::init();

    // START
    let context = test_utilities::context();

    let mut environment = test_utilities::working_environment(&context);
    let ea = EnvironmentAction::Environment(environment);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    // PAUSE
    let context = test_utilities::context();

    let mut environment = test_utilities::working_environment(&context);
    let ea = EnvironmentAction::Environment(environment);

    match pause_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    // START
    let context = test_utilities::context();

    let mut environment = test_utilities::working_environment(&context);
    let ea = EnvironmentAction::Environment(environment);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    // DELETE
    let context = test_utilities::context();

    let mut environment = test_utilities::working_environment(&context);
    let ea = EnvironmentAction::Environment(environment);

    match delete_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

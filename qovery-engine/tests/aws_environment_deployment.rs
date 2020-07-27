extern crate test_utilities;

use qovery_engine::models::EnvironmentAction;
use qovery_engine::transaction::TransactionResult;

fn do_deployment(execution_id: &str, environment_action: &EnvironmentAction) -> TransactionResult {
    let engine = test_utilities::docker_ecr_aws_engine(execution_id);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let cp = test_utilities::cloud_provider_aws(execution_id);
    let nodes = test_utilities::aws_kubernetes_nodes();

    let k = test_utilities::aws_kubernetes_eks(execution_id, &cp, nodes);

    tx.deploy_environment(&k, &environment_action);

    tx.commit()
}

#[test]
fn deploy_a_working_environment_with_all_options_on_aws_eks() {
    let execution_id = test_utilities::execution_id();

    let ea =
        EnvironmentAction::Environment(test_utilities::working_environment(execution_id.as_str()));

    match do_deployment(execution_id.as_str(), &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
fn deploy_a_working_environment_with_no_router_on_aws_eks() {
    let execution_id = test_utilities::execution_id();

    let mut environment = test_utilities::working_environment(execution_id.as_str());

    environment.routers = vec![];

    let ea = EnvironmentAction::Environment(environment);

    match do_deployment(execution_id.as_str(), &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
fn deploy_a_working_environment_with_no_database_on_aws_eks() {
    // TODO
}

#[test]
fn deploy_a_working_environment_with_no_storage_on_aws_eks() {
    // TODO
}

#[test]
fn deploy_a_working_environment_with_no_custom_domain_on_aws_eks() {
    // TODO
}

#[test]
fn deploy_a_non_working_environment_with_no_failover_on_aws_eks() {
    // TODO
}

#[test]
fn deploy_a_non_working_environment_with_a_working_failover_on_aws_eks() {
    // TODO
}

#[test]
fn deploy_a_non_working_environment_with_a_non_working_failover_on_aws_eks() {
    // TODO
}

#[test]
fn deploy_a_working_environment_with_a_failing_default_domain_on_aws_eks() {
    // TODO
}

#[test]
fn deploy_but_fail_to_push_image_on_container_registry() {
    // TODO
}

#[test]
fn delete_a_working_environment_on_aws_eks() {
    // TODO
}

#[test]
fn delete_a_non_working_environment_on_aws_eks() {
    // TODO
}

#[test]
fn deploy_and_delete_and_deploy_a_working_environment_on_aws_eks() {
    // TODO
}

#[test]
fn pause_a_working_environment_on_aws_eks() {
    // TODO
}

#[test]
fn pause_a_non_working_environment_on_aws_eks() {
    // TODO
}

#[test]
fn pause_and_start_a_working_environment_on_aws_eks() {
    // TODO
}

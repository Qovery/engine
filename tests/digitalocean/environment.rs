use qovery_engine::models::{Action, Clone2, EnvironmentAction};
use qovery_engine::transaction::TransactionResult;
use test_utilities::utilities::{context, init};
use crate::digitalocean::deploy_environment_on_do;
use tracing::{debug, error, info, span, warn, Level};

#[test]
fn deploy_a_working_environment_with_no_router_on_do() {
    init();
    let span = span!(
        Level::INFO,
        "deploy_a_working_environment_with_no_router_on_do"
    );
    let _enter = span.enter();

    let context = context();
    let context_for_delete = context.clone_not_same_execution_id();
    let mut environment = test_utilities::aws::environment_only_http_server(&context);
    let mut environment_for_delete = test_utilities::aws::environment_only_http_server(&context);
    environment.routers = vec![];
    environment_for_delete.routers = vec![];
    environment_for_delete.action = Action::Delete;
    let ea = EnvironmentAction::Environment(environment);
    let ea_delete = EnvironmentAction::Environment(environment_for_delete);

    match deploy_environment_on_do(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    /*
    TODO: delete environement is not implemented yet
    match deploy_environment_on_do(&context_for_delete, &ea_delete) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };*/
}

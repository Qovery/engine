use test_utilities::utilities::{context, init, engine_run_test};
use tracing::{span, Level};

use qovery_engine::models::{Action, Clone2, EnvironmentAction};
use qovery_engine::transaction::TransactionResult;

use crate::digitalocean::deploy_environment_on_do;

//TODO: Do you wanna play a game ?
fn deploy_one_postgresql() {
    engine_run_test(|| {

        let span = span!(Level::INFO, "deploy_one_postgresql");
        let _enter = span.enter();

        let context = context();
        let context_for_deletion = context.clone_not_same_execution_id();

        let mut environment = test_utilities::aws::working_minimal_environment(&context);

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;
        let ea = EnvironmentAction::Environment(environment);
        let ea_delete = EnvironmentAction::Environment(environment_delete);

        match deploy_environment_on_do(&context, &ea) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        match deploy_environment_on_do(&context_for_deletion, &ea_delete) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };
        return "deploy_one_postgresql".to_string();
    })
}

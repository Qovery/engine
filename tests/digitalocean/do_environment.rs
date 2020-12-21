use crate::digitalocean::deploy_environment_on_do;
use qovery_engine::build_platform::Image;
use qovery_engine::container_registry::docr::get_current_registry_name;
use qovery_engine::models::{Action, Clone2, EnvironmentAction};
use qovery_engine::transaction::TransactionResult;
use test_utilities::digitalocean::digital_ocean_token;
use test_utilities::utilities::{context, init};
use tracing::{debug, error, info, span, warn, Level};

// this function tests DOCR as well
// #[test]
// #[ignore]
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
    let ea = EnvironmentAction::Environment(environment.clone());
    let ea_delete = EnvironmentAction::Environment(environment_for_delete.clone());

    let engine = test_utilities::digitalocean::docker_cr_do_engine(&context);
    let registry = engine.container_registry();
    let image = Image {
        application_id: "".to_string(),
        name: environment.applications.first().unwrap().name.clone(),
        tag: environment.applications.first().unwrap().commit_id.clone(),
        commit_id: "".to_string(),
        registry_url: None,
    };
    assert!(!registry.does_image_exists(&image));

    match deploy_environment_on_do(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    let registry_name = registry.name();
    assert_eq!(
        registry_name,
        get_current_registry_name(digital_ocean_token().as_str())
            .unwrap()
            .as_str()
    );
    assert!(registry.does_image_exists(&image));
    /*
    TODO: delete environement is not implemented yet
    match deploy_environment_on_do(&context_for_delete, &ea_delete) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };*/
}

// #[test]
// #[ignore]
fn deploy_a_working_environment_router_and_app_on_do() {
    init();

    let span = span!(
        Level::INFO,
        "deploy_a_working_environment_router_and_app_on_do"
    );
    let _enter = span.enter();

    let context = context();
    let context_for_delete = context.clone_not_same_execution_id();
    let mut environment = test_utilities::aws::environment_only_http_server_router(&context);
    let ea = EnvironmentAction::Environment(environment);

    match deploy_environment_on_do(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

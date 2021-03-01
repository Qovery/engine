extern crate test_utilities;

use self::test_utilities::cloudflare::dns_provider_cloudflare;
use crate::digitalocean::deploy_environment_on_do;
use qovery_engine::build_platform::Image;
use qovery_engine::container_registry::docr::get_current_registry_name;
use qovery_engine::models::{Action, Clone2, Context, CustomDomain, EnvironmentAction};
use qovery_engine::transaction::{DeploymentOption, TransactionResult};
use test_utilities::digitalocean::digital_ocean_token;
use test_utilities::utilities::{context, engine_run_test};
use tracing::{span, Level};

pub fn deploy_environment(context: &Context, environment_action: &EnvironmentAction) -> TransactionResult {
    let engine = test_utilities::digitalocean::docker_cr_do_engine(&context);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let cp = test_utilities::digitalocean::cloud_provider_digitalocean(&context);
    let nodes = test_utilities::digitalocean::do_kubernetes_nodes();
    let dns_provider = dns_provider_cloudflare(context);
    let k = test_utilities::digitalocean::do_kubernetes_ks(&context, &cp, &dns_provider, nodes);

    let _ = tx.deploy_environment_with_options(
        &k,
        &environment_action,
        DeploymentOption {
            force_build: true,
            force_push: true,
        },
    );

    tx.commit()
}

pub fn delete_environment(context: &Context, environment_action: &EnvironmentAction) -> TransactionResult {
    let engine = test_utilities::digitalocean::docker_cr_do_engine(&context);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let cp = test_utilities::digitalocean::cloud_provider_digitalocean(&context);
    let nodes = test_utilities::digitalocean::do_kubernetes_nodes();
    let dns_provider = dns_provider_cloudflare(context);
    let k = test_utilities::digitalocean::do_kubernetes_ks(&context, &cp, &dns_provider, nodes);

    let _ = tx.delete_environment(&k, &environment_action);

    tx.commit()
}

// this function tests DOCR as well
//#[test]
fn deploy_a_working_environment_with_no_router_on_do() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "deploy_a_working_environment_with_no_router_on_do");
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
            registry_name: None,
            registry_secret: None,
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
        TODO: delete environment is not implemented yet
        match deploy_environment_on_do(&context_for_delete, &ea_delete) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };*/

        return "deploy_a_working_environment_with_no_router_on_do".to_string();
    })
}

//#[test]
fn do_deploy_a_working_environment_with_custom_domain() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "deploy_a_working_environment_with_custom_domain");
        let _enter = span.enter();

        let context = context();
        let context_for_delete = context.clone_not_same_execution_id();

        let mut environment = test_utilities::aws::working_minimal_environment(&context);
        // Todo: fix domains
        environment.routers = environment
            .routers
            .into_iter()
            .map(|mut router| {
                router.custom_domains = vec![CustomDomain {
                    // should be the client domain
                    domain: "test-domain.qvy.io".to_string(),
                    // should be our domain
                    target_domain: "target-domain.oom.sh".to_string(),
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

        // todo: check TLS

        match delete_environment(&context_for_delete, &ea_delete) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };
        return "do_deploy_a_working_environment_with_custom_domain".to_string();
    })
}

//#[test]
fn deploy_a_working_environment_router_and_app_on_do() {
    engine_run_test(|| {
        let span = span!(Level::INFO, "deploy_a_working_environment_router_and_app_on_do");
        let _enter = span.enter();

        let context = context();
        //let context_for_delete = context.clone_not_same_execution_id();
        let environment = test_utilities::aws::environment_only_http_server_router(&context);
        let ea = EnvironmentAction::Environment(environment);

        match deploy_environment_on_do(&context, &ea) {
            TransactionResult::Ok => assert!(true),
            TransactionResult::Rollback(_) => assert!(false),
            TransactionResult::UnrecoverableError(_, _) => assert!(false),
        };

        return "deploy_a_working_environment_router_and_app_on_do".to_string();
    })
}

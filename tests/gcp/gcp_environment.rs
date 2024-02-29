use crate::helpers;
use crate::helpers::common::Infrastructure;
use crate::helpers::gcp::{clean_environments, gcp_default_infra_config};
use crate::helpers::utilities::{
    context_for_resource, engine_run_test, get_pods, init, logger, metrics_registry, FuncTestsSecrets,
};
use function_name::named;
use qovery_engine::cloud_provider::gcp::locations::GcpRegion;
use qovery_engine::cloud_provider::Kind;
use qovery_engine::io_models::application::{Port, Protocol};
use qovery_engine::io_models::context::CloneForTest;
use qovery_engine::io_models::router::CustomDomain;
use qovery_engine::io_models::Action;
use qovery_engine::transaction::TransactionResult;
use std::str::FromStr;
use tracing::log::warn;
use tracing::span;
use tracing::Level;
use uuid::Uuid;

#[cfg(feature = "test-gcp-minimal")]
#[named]
#[test]
fn gcp_test_build_phase() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .GCP_TEST_ORGANIZATION_LONG_ID
                .expect("GCP_TEST_ORGANIZATION_LONG_ID"),
            secrets.GCP_TEST_CLUSTER_LONG_ID.expect("GCP_TEST_CLUSTER_LONG_ID"),
        );
        let infra_ctx = gcp_default_infra_config(&context, logger.clone(), metrics_registry.clone());
        let environment = helpers::environment::working_minimal_environment(&context);

        let env_action = environment.clone();

        let (env, ret) = environment.build_environment(&env_action, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        // Check the image exist in the registry
        let img_exist = infra_ctx
            .container_registry()
            .image_exists(&env.applications[0].get_build().image);
        assert!(img_exist);

        // TODO(benjaminch): add clean registry

        test_name.to_string()
    })
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn gcp_gke_deploy_a_working_environment_with_no_router() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .GCP_TEST_ORGANIZATION_LONG_ID
                .expect("GCP_TEST_ORGANIZATION_LONG_ID"),
            secrets.GCP_TEST_CLUSTER_LONG_ID.expect("GCP_TEST_CLUSTER_LONG_ID"),
        );
        let region = GcpRegion::from_str(
            secrets
                .GCP_DEFAULT_REGION
                .as_ref()
                .expect("GCP_DEFAULT_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown GCP region");
        let infra_ctx = gcp_default_infra_config(&context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete =
            gcp_default_infra_config(&context_for_delete, logger.clone(), metrics_registry.clone());
        let environment = helpers::environment::working_minimal_environment(&context);

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let env_action = environment.clone();
        let env_action_for_delete = environment_for_delete.clone();

        let result = environment.deploy_environment(&env_action, &infra_ctx);
        assert!(matches!(result, TransactionResult::Ok));

        let result = environment_for_delete.delete_environment(&env_action_for_delete, &infra_ctx_for_delete);
        assert!(matches!(result, TransactionResult::Ok));

        if let Err(e) = clean_environments(&context, vec![environment], secrets, region) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn gcp_gke_deploy_a_not_working_environment_with_no_router() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name,);
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .GCP_TEST_ORGANIZATION_LONG_ID
                .expect("GCP_TEST_ORGANIZATION_LONG_ID"),
            secrets.GCP_TEST_CLUSTER_LONG_ID.expect("GCP_TEST_CLUSTER_LONG_ID"),
        );
        let region = GcpRegion::from_str(
            secrets
                .GCP_DEFAULT_REGION
                .as_ref()
                .expect("GCP_DEFAULT_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown GCP region");
        let infra_ctx = gcp_default_infra_config(&context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete =
            gcp_default_infra_config(&context_for_delete, logger.clone(), metrics_registry.clone());

        let mut environment = helpers::environment::non_working_environment(&context);
        environment.routers = vec![];

        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        let env_action = environment.clone();
        let env_action_for_delete = environment_for_delete.clone();

        let result = environment.deploy_environment(&env_action, &infra_ctx);
        assert!(matches!(result, TransactionResult::Error(_)));

        let result = environment_for_delete.delete_environment(&env_action_for_delete, &infra_ctx_for_delete);
        assert!(matches!(result, TransactionResult::Ok | TransactionResult::Error(_)));

        if let Err(e) = clean_environments(&context, vec![environment], secrets, region) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn gcp_gke_deploy_a_working_environment_and_pause() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .GCP_TEST_ORGANIZATION_LONG_ID
                .expect("GCP_TEST_ORGANIZATION_LONG_ID"),
            secrets.GCP_TEST_CLUSTER_LONG_ID.expect("GCP_TEST_CLUSTER_LONG_ID"),
        );
        let region = GcpRegion::from_str(
            secrets
                .GCP_DEFAULT_REGION
                .as_ref()
                .expect("GCP_DEFAULT_REGION is not set")
                .to_string()
                .as_str(),
        )
        .expect("Unknown GCP region");
        let infra_ctx = gcp_default_infra_config(&context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete =
            gcp_default_infra_config(&context_for_delete, logger.clone(), metrics_registry.clone());
        let environment = helpers::environment::working_minimal_environment(&context);

        let env_action = environment.clone();
        let selector = &environment.applications[0].long_id;

        let result = environment.deploy_environment(&env_action, &infra_ctx);
        assert!(matches!(result, TransactionResult::Ok));

        let ret = get_pods(&infra_ctx, Kind::Gcp, &environment, selector, secrets.clone());
        assert!(ret.is_ok());
        assert!(!ret.unwrap().items.is_empty());

        let result = environment.pause_environment(&env_action, &infra_ctx_for_delete);
        assert!(matches!(result, TransactionResult::Ok));

        // Check that we have actually 0 pods running for this app
        let ret = get_pods(&infra_ctx, Kind::Gcp, &environment, selector, secrets.clone());
        assert!(ret.is_ok());
        assert!(ret.unwrap().items.is_empty());

        // Check we can resume the env
        let ctx_resume = context.clone_not_same_execution_id();
        let infra_ctx_resume = gcp_default_infra_config(&ctx_resume, logger.clone(), metrics_registry.clone());
        let result = environment.deploy_environment(&env_action, &infra_ctx_resume);
        assert!(matches!(result, TransactionResult::Ok));

        let ret = get_pods(&infra_ctx, Kind::Gcp, &environment, selector, secrets.clone());
        assert!(ret.is_ok());
        assert!(!ret.unwrap().items.is_empty());

        // Cleanup
        let result = environment.delete_environment(&env_action, &infra_ctx_for_delete);
        assert!(matches!(result, TransactionResult::Ok));

        if let Err(e) = clean_environments(&context, vec![environment], secrets, region) {
            warn!("cannot clean environments, error: {:?}", e);
        }

        test_name.to_string()
    })
}

#[cfg(feature = "test-gcp-self-hosted")]
#[named]
#[test]
fn gcp_gke_deploy_a_working_environment_with_domain() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test",);
        let _enter = span.enter();

        let logger = logger();
        let metrics_registry = metrics_registry();
        let secrets = FuncTestsSecrets::new();
        let context = context_for_resource(
            secrets
                .GCP_TEST_ORGANIZATION_LONG_ID
                .expect("GCP_TEST_ORGANIZATION_LONG_ID"),
            secrets.GCP_TEST_CLUSTER_LONG_ID.expect("GCP_TEST_CLUSTER_LONG_ID"),
        );
        let infra_ctx = gcp_default_infra_config(&context, logger.clone(), metrics_registry.clone());
        let context_for_delete = context.clone_not_same_execution_id();
        let infra_ctx_for_delete =
            gcp_default_infra_config(&context_for_delete, logger.clone(), metrics_registry.clone());
        let mut environment = helpers::environment::working_minimal_environment_with_router(
            &context,
            secrets
                .DEFAULT_TEST_DOMAIN
                .as_ref()
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

        let mut modified_environment = environment.clone();
        modified_environment.applications.clear();
        modified_environment.routers.clear();

        for (idx, router) in environment.routers.into_iter().enumerate() {
            // add custom domain
            let mut router = router.clone();
            let cd = CustomDomain {
                domain: format!("fake-custom-domain-{idx}.qovery.io"),
                target_domain: format!("validation-domain-{idx}"),
                generate_certificate: true,
            };

            router.custom_domains = vec![cd];
            modified_environment.routers.push(router);
        }

        for mut application in environment.applications.into_iter() {
            application.ports.push(Port {
                long_id: Uuid::new_v4(),
                port: 5050,
                is_default: false,
                name: "grpc".to_string(),
                publicly_accessible: true,
                protocol: Protocol::GRPC,
                service_name: None,
                namespace: None,
            });
            // disable custom domain check
            application.advanced_settings.deployment_custom_domain_check_enabled = false;
            modified_environment.applications.push(application);
        }

        environment = modified_environment;

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;

        let ea = environment.clone();
        let ea_delete = environment_delete.clone();

        let ret = environment.deploy_environment(&ea, &infra_ctx);
        assert!(matches!(ret, TransactionResult::Ok));

        let ret = environment_delete.delete_environment(&ea_delete, &infra_ctx_for_delete);
        assert!(matches!(ret, TransactionResult::Ok));

        test_name.to_string()
    })
}

pub use qovery_engine::cloud_provider::Kind;
pub use std::collections::BTreeMap;

pub use crate::helpers::aws::{
    aws_kubernetes_eks, aws_kubernetes_nodes, cloud_provider_aws, docker_ecr_aws_engine, AWS_KUBE_TEST_CLUSTER_ID,
    AWS_QOVERY_ORGANIZATION_ID,
};
pub use crate::helpers::cloudflare::dns_provider_cloudflare;
pub use crate::helpers::common::{echo_app_environment, non_working_environment, working_minimal_environment};
pub use crate::helpers::utilities::{
    context, engine_run_test, generate_id, get_pods, init, is_pod_restarted_env, FuncTestsSecrets,
};
pub use function_name::named;
pub use qovery_engine::models::{Action, Clone2, Context, EnvironmentAction, Storage, StorageType};
pub use qovery_engine::transaction::{DeploymentOption, TransactionResult};
pub use tracing::{span, Level};

// TODO:
//   - Tests that applications are always restarted when recieving a CREATE action
//     see: https://github.com/Qovery/engine/pull/269

#[cfg(test)]
pub fn deploy_environment(context: &Context, environment_action: &EnvironmentAction) -> TransactionResult {
    let engine = docker_ecr_aws_engine(context);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let cp = cloud_provider_aws(context);
    let nodes = aws_kubernetes_nodes();
    let dns_provider = dns_provider_cloudflare(context);
    let k = aws_kubernetes_eks(context, &cp, &dns_provider, nodes);

    let _ = tx.deploy_environment_with_options(
        &k,
        environment_action,
        DeploymentOption {
            force_build: true,
            force_push: true,
        },
    );

    tx.commit()
}

#[cfg(test)]
pub fn ctx_pause_environment(context: &Context, environment_action: &EnvironmentAction) -> TransactionResult {
    let engine = docker_ecr_aws_engine(context);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let cp = cloud_provider_aws(context);
    let nodes = aws_kubernetes_nodes();
    let dns_provider = dns_provider_cloudflare(context);
    let k = aws_kubernetes_eks(context, &cp, &dns_provider, nodes);

    let _ = tx.pause_environment(&k, environment_action);

    tx.commit()
}

#[cfg(test)]
pub fn delete_environment(context: &Context, environment_action: &EnvironmentAction) -> TransactionResult {
    let engine = docker_ecr_aws_engine(context);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let cp = cloud_provider_aws(context);
    let nodes = aws_kubernetes_nodes();
    let dns_provider = dns_provider_cloudflare(context);
    let k = aws_kubernetes_eks(context, &cp, &dns_provider, nodes);

    let _ = tx.delete_environment(&k, environment_action);

    tx.commit()
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_a_working_environment_with_no_router_on_aws_eks() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let context = context();
        let context_for_delete = context.clone_not_same_execution_id();
        let secrets = FuncTestsSecrets::new();
        let mut environment = working_minimal_environment(
            &context,
            AWS_QOVERY_ORGANIZATION_ID,
            secrets
                .DEFAULT_TEST_DOMAIN
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

        let mut environment_for_delete = environment.clone();
        environment.routers = vec![];
        environment_for_delete.routers = vec![];
        environment_for_delete.action = Action::Delete;

        let ea = EnvironmentAction::Environment(environment);
        let ea_delete = EnvironmentAction::Environment(environment_for_delete);

        match deploy_environment(&context, &ea) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        match delete_environment(&context_for_delete, &ea_delete) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        return test_name.to_string();
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_a_working_environment_and_pause_it_eks() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let context = context();
        let context_for_delete = context.clone_not_same_execution_id();
        let secrets = FuncTestsSecrets::new();
        let environment = working_minimal_environment(
            &context,
            AWS_QOVERY_ORGANIZATION_ID,
            secrets
                .DEFAULT_TEST_DOMAIN
                .as_ref()
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

        let ea = EnvironmentAction::Environment(environment.clone());
        match deploy_environment(&context, &ea) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        match ctx_pause_environment(&context_for_delete, &ea) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        // Check that we have actually 0 pods running for this app
        let app_name = format!("{}-0", environment.applications[0].name);
        let ret = get_pods(
            Kind::Aws,
            environment.clone(),
            app_name.clone().as_str(),
            AWS_KUBE_TEST_CLUSTER_ID,
            secrets.clone(),
        );
        assert_eq!(ret.is_ok(), true);
        assert_eq!(ret.unwrap().items.is_empty(), true);

        // Check we can resume the env
        let ctx_resume = context.clone_not_same_execution_id();
        match deploy_environment(&ctx_resume, &ea) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        // Cleanup
        match delete_environment(&context_for_delete, &ea) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };
        return test_name.to_string();
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_a_not_working_environment_with_no_router_on_aws_eks() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();
        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let context = context();
        let context_for_delete = context.clone_not_same_execution_id();
        let secrets = FuncTestsSecrets::new();

        let mut environment = non_working_environment(
            &context,
            AWS_QOVERY_ORGANIZATION_ID,
            secrets
                .DEFAULT_TEST_DOMAIN
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );
        environment.routers = vec![];

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;

        let ea = EnvironmentAction::Environment(environment);
        let ea_delete = EnvironmentAction::Environment(environment_delete);

        match deploy_environment(&context, &ea) {
            TransactionResult::Ok => panic!(),
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => {}
        };

        match delete_environment(&context_for_delete, &ea_delete) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => {}
        };

        return test_name.to_string();
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn build_with_buildpacks_and_deploy_a_working_environment() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let context = context();
        let context_for_deletion = context.clone_not_same_execution_id();
        let secrets = FuncTestsSecrets::new();
        let mut environment = working_minimal_environment(
            &context,
            AWS_QOVERY_ORGANIZATION_ID,
            secrets
                .DEFAULT_TEST_DOMAIN
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );
        environment.applications = environment
            .applications
            .into_iter()
            .map(|mut app| {
                app.private_port = Some(3000);
                app.commit_id = "f59237d603829636138e2f22a0549e33b5dd6e1f".to_string();
                app.branch = "simple-node-app".to_string();
                app.dockerfile_path = None;
                app
            })
            .collect::<Vec<qovery_engine::models::Application>>();

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;

        let ea = EnvironmentAction::Environment(environment);
        let ea_delete = EnvironmentAction::Environment(environment_delete);

        match deploy_environment(&context, &ea) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        match delete_environment(&context_for_deletion, &ea_delete) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        return test_name.to_string();
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn build_worker_with_buildpacks_and_deploy_a_working_environment() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let context = context();
        let context_for_deletion = context.clone_not_same_execution_id();
        let secrets = FuncTestsSecrets::new();
        let mut environment = working_minimal_environment(
            &context,
            AWS_QOVERY_ORGANIZATION_ID,
            secrets
                .DEFAULT_TEST_DOMAIN
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );
        environment.applications = environment
            .applications
            .into_iter()
            .map(|mut app| {
                app.private_port = None;
                app.commit_id = "4f35f4ab3e98426c5a3eaa91e788ff8ab466f19a".to_string();
                app.branch = "buildpack-process".to_string();
                app.dockerfile_path = None;
                app
            })
            .collect::<Vec<qovery_engine::models::Application>>();

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;

        let ea = EnvironmentAction::Environment(environment);
        let ea_delete = EnvironmentAction::Environment(environment_delete);

        match deploy_environment(&context, &ea) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        match delete_environment(&context_for_deletion, &ea_delete) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        return test_name.to_string();
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_a_working_environment_with_domain() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let context = context();
        let context_for_deletion = context.clone_not_same_execution_id();
        let secrets = FuncTestsSecrets::new();
        let environment = working_minimal_environment(
            &context,
            AWS_QOVERY_ORGANIZATION_ID,
            secrets
                .DEFAULT_TEST_DOMAIN
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;

        let ea = EnvironmentAction::Environment(environment);
        let ea_delete = EnvironmentAction::Environment(environment_delete);

        match deploy_environment(&context, &ea) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        match delete_environment(&context_for_deletion, &ea_delete) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        return test_name.to_string();
    })
}

// todo: test it
// fn deploy_a_working_environment_with_custom_domain() {
//     engine_run_test(|| {
//         let span = span!(
//             Level::INFO,
//             "test",
//             name = "deploy_a_working_environment_with_custom_domain"
//         );
//         let _enter = span.enter();
//
//         let context = context();
//         let context_for_delete = context.clone_not_same_execution_id();
//         let secrets = FuncTestsSecrets::new();
//
//         let mut environment = working_minimal_environment(&context, secrets.clone());
//         // Todo: fix domains
//         environment.routers = environment
//             .routers
//             .into_iter()
//             .map(|mut router| {
//                 router.custom_domains = vec![CustomDomain {
//                     // should be the client domain
//                     domain: format!("test-domain.{}", secrets.clone().CUSTOM_TEST_DOMAIN.unwrap()),
//                     // should be our domain
//                     target_domain: format!("target-domain.{}", secrets.clone().DEFAULT_TEST_DOMAIN.unwrap()),
//                 }];
//                 router
//             })
//             .collect::<Vec<qovery_engine::models::Router>>();
//
//         let mut environment_delete = environment.clone();
//         environment_delete.action = Action::Delete;
//
//         let ea = EnvironmentAction::Environment(environment);
//         let ea_delete = EnvironmentAction::Environment(environment_delete);
//
//         match deploy_environment(&context, &ea) {
//             TransactionResult::Ok => {},
//             TransactionResult::Rollback(_) => panic!(),
//             TransactionResult::UnrecoverableError(_, _) => panic!(),
//         };
//
//         // todo: check TLS
//
//         match delete_environment(&context_for_delete, &ea_delete) {
//             TransactionResult::Ok => {},
//             TransactionResult::Rollback(_) => panic!(),
//             TransactionResult::UnrecoverableError(_, _) => panic!(),
//         };
//         return "deploy_a_working_environment_with_custom_domain".to_string();
//     })
// }

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_a_working_environment_with_storage_on_aws_eks() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let context = context();
        let context_for_deletion = context.clone_not_same_execution_id();
        let secrets = FuncTestsSecrets::new();

        let mut environment = working_minimal_environment(
            &context,
            AWS_QOVERY_ORGANIZATION_ID,
            secrets
                .DEFAULT_TEST_DOMAIN
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

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
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        // todo: check the disk is here and with correct size

        match delete_environment(&context_for_deletion, &ea_delete) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        return test_name.to_string();
    })
}

// to check if app redeploy or not, it shouldn't
#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn redeploy_same_app_with_ebs() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let context = context();
        let context_bis = context.clone_not_same_execution_id();
        let context_for_deletion = context.clone_not_same_execution_id();
        let secrets = FuncTestsSecrets::new();

        let mut environment = working_minimal_environment(
            &context,
            AWS_QOVERY_ORGANIZATION_ID,
            secrets
                .DEFAULT_TEST_DOMAIN
                .as_ref()
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

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
        let environment_redeploy = environment.clone();
        let environment_check1 = environment.clone();
        let environment_check2 = environment.clone();
        let mut environment_delete = environment.clone();
        environment_delete.action = Action::Delete;

        let ea = EnvironmentAction::Environment(environment);
        let ea2 = EnvironmentAction::Environment(environment_redeploy);
        let ea_delete = EnvironmentAction::Environment(environment_delete);

        match deploy_environment(&context, &ea) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };
        let app_name = format!("{}-0", &environment_check1.applications[0].name);
        let (_, number) = is_pod_restarted_env(
            Kind::Aws,
            AWS_KUBE_TEST_CLUSTER_ID,
            environment_check1,
            app_name.clone().as_str(),
            secrets.clone(),
        );

        match deploy_environment(&context_bis, &ea2) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        let (_, number2) = is_pod_restarted_env(
            Kind::Aws,
            AWS_KUBE_TEST_CLUSTER_ID,
            environment_check2,
            app_name.as_str(),
            secrets,
        );
        //nothing change in the app, so, it shouldn't be restarted
        assert!(number.eq(&number2));
        match delete_environment(&context_for_deletion, &ea_delete) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };
        return test_name.to_string();
    })
}

// #[test]
// fn deploy_a_working_environment_with_external_service() {
//     init();
//
//     let context = context();
//     let deletion_context = context.clone_not_same_execution_id();
//
//     let mut environment = working_minimal_environment(&context);
//
//     // no apps
//     environment.applications = vec![];
//
//     environment.external_services = vec![ExternalService {
//         id: generate_id(),
//         action: Action::Create,
//         name: "my-external-service".to_string(),
//         total_cpus: "500m".to_string(),
//         total_ram_in_mib: 512,
//         git_url: "https://github.com/evoxmusic/qovery-external-service-example.git".to_string(),
//         git_credentials: GitCredentials {
//             login: "x-access-token".to_string(),
//             access_token: "CHANGE ME".to_string(), // fake one
//             expired_at: Utc::now(),
//         },
//         branch: "master".to_string(),
//         commit_id: "db322f2f4ac70933f16e8a422ea9f72e1e14df22".to_string(),
//         on_create_dockerfile_path: "extsvc/Dockerfile.on-create".to_string(),
//         on_pause_dockerfile_path: "extsvc/Dockerfile.on-pause".to_string(),
//         on_delete_dockerfile_path: "extsvc/Dockerfile.on-delete".to_string(),
//         environment_variables: vec![],
//     }];
//
//     let mut environment_delete = environment.clone();
//     environment_delete.action = Action::Delete;
//
//     let ea = EnvironmentAction::Environment(environment);
//     let ea_delete = EnvironmentAction::Environment(environment_delete);
//
//     match deploy_environment(&context, &ea) {
//         TransactionResult::Ok => {},
//         TransactionResult::Rollback(_) => panic!(),
//         TransactionResult::UnrecoverableError(_, _) => panic!(),
//     };
//
//     match delete_environment(&deletion_context, &ea_delete) {
//         TransactionResult::Ok => {},
//         TransactionResult::Rollback(_) => panic!(),
//         TransactionResult::UnrecoverableError(_, _) => panic!(),
//     };
//
//     // TODO: remove the namespace (or project)
// }

/*#[test]
#[ignore]
fn deploy_a_working_production_environment_with_all_options_on_aws_eks() {
    init();

    let context = context();

    let mut environment = working_environment(&context);
    environment.kind = Kind::Production;
    let environment_delete = environment.clone_not_same_execution_id();
    let ea = EnvironmentAction::Environment(environment);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => {},
        TransactionResult::Rollback(_) => panic!(),
        TransactionResult::UnrecoverableError(_, _) => panic!(),
    };
    let ea_delete = EnvironmentAction::Environment(environment_delete);
    match delete_environment(&context, &ea_delete) {
        TransactionResult::Ok => {},
        TransactionResult::Rollback(_) => panic!(),
        TransactionResult::UnrecoverableError(_, _) => panic!(),
    };
}*/

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_a_not_working_environment_and_after_working_environment() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        // let mut contex_envs = generate_contexts_and_environments(3, working_minimal_environment);
        let context = context();
        let context_for_not_working = context.clone_not_same_execution_id();
        let context_for_delete = context.clone_not_same_execution_id();
        let secrets = FuncTestsSecrets::new();

        // env part generation
        let environment = working_minimal_environment(
            &context,
            AWS_QOVERY_ORGANIZATION_ID,
            secrets
                .DEFAULT_TEST_DOMAIN
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );
        let mut environment_for_not_working = environment.clone();
        // this environment is broken by container exit
        environment_for_not_working.applications = environment_for_not_working
            .applications
            .into_iter()
            .map(|mut app| {
                app.git_url = "https://github.com/Qovery/engine-testing.git".to_string();
                app.branch = "1app_fail_deploy".to_string();
                app.commit_id = "5b89305b9ae8a62a1f16c5c773cddf1d12f70db1".to_string();
                app.environment_vars = BTreeMap::default();
                app
            })
            .collect::<Vec<qovery_engine::models::Application>>();
        let mut environment_for_delete = environment.clone();
        environment_for_delete.action = Action::Delete;

        // environment actions
        let ea = EnvironmentAction::Environment(environment);
        let ea_not_working = EnvironmentAction::Environment(environment_for_not_working);
        let ea_delete = EnvironmentAction::Environment(environment_for_delete);

        match deploy_environment(&context_for_not_working, &ea_not_working) {
            TransactionResult::Ok => panic!(),
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => {}
        };
        match deploy_environment(&context, &ea) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };
        match delete_environment(&context_for_delete, &ea_delete) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        return test_name.to_string();
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[test]
#[ignore]
#[named]
fn deploy_ok_fail_fail_ok_environment() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = "deploy_ok_fail_fail_ok_environment");
        let _enter = span.enter();

        // working env
        let context = context();
        let secrets = FuncTestsSecrets::new();
        let environment = working_minimal_environment(
            &context,
            AWS_QOVERY_ORGANIZATION_ID,
            secrets
                .DEFAULT_TEST_DOMAIN
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

        // not working 1
        let context_for_not_working_1 = context.clone_not_same_execution_id();
        let mut not_working_env_1 = environment.clone();
        not_working_env_1.applications = not_working_env_1
            .applications
            .into_iter()
            .map(|mut app| {
                app.git_url = "https://gitlab.com/maathor/my-exit-container".to_string();
                app.branch = "master".to_string();
                app.commit_id = "55bc95a23fbf91a7699c28c5f61722d4f48201c9".to_string();
                app.environment_vars = BTreeMap::default();
                app
            })
            .collect::<Vec<qovery_engine::models::Application>>();

        // not working 2
        let context_for_not_working_2 = context.clone_not_same_execution_id();
        let not_working_env_2 = not_working_env_1.clone();

        // work for delete
        let context_for_delete = context.clone_not_same_execution_id();
        let mut delete_env = environment.clone();
        delete_env.action = Action::Delete;

        let ea = EnvironmentAction::Environment(environment);
        let ea_not_working_1 = EnvironmentAction::Environment(not_working_env_1);
        let ea_not_working_2 = EnvironmentAction::Environment(not_working_env_2);
        let ea_delete = EnvironmentAction::Environment(delete_env);

        // OK
        match deploy_environment(&context, &ea) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        // FAIL and rollback
        match deploy_environment(&context_for_not_working_1, &ea_not_working_1) {
            TransactionResult::Ok => panic!(),
            TransactionResult::Rollback(_) => {}
            TransactionResult::UnrecoverableError(_, _) => {}
        };

        // FAIL and Rollback again
        match deploy_environment(&context_for_not_working_2, &ea_not_working_2) {
            TransactionResult::Ok => panic!(),
            TransactionResult::Rollback(_) => {}
            TransactionResult::UnrecoverableError(_, _) => {}
        };

        // Should be working
        match deploy_environment(&context, &ea) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        match delete_environment(&context_for_delete, &ea_delete) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        return test_name.to_string();
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_a_non_working_environment_with_no_failover_on_aws_eks() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let context = context();
        let secrets = FuncTestsSecrets::new();
        let environment = non_working_environment(
            &context,
            AWS_QOVERY_ORGANIZATION_ID,
            secrets
                .DEFAULT_TEST_DOMAIN
                .expect("DEFAULT_TEST_DOMAIN is not set in secrets")
                .as_str(),
        );

        let context_for_delete = context.clone_not_same_execution_id();
        let mut delete_env = environment.clone();
        delete_env.action = Action::Delete;

        let ea = EnvironmentAction::Environment(environment);
        let ea_delete = EnvironmentAction::Environment(delete_env);

        match deploy_environment(&context, &ea) {
            TransactionResult::Ok => panic!(),
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => {}
        };
        match delete_environment(&context_for_delete, &ea_delete) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        return test_name.to_string();
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_a_non_working_environment_with_a_working_failover_on_aws_eks() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        // context for non working environment
        let context = context();
        let secrets = FuncTestsSecrets::new();
        let test_domain = secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets");

        let environment = non_working_environment(&context, AWS_QOVERY_ORGANIZATION_ID, test_domain.as_str());
        let failover_environment =
            working_minimal_environment(&context, AWS_QOVERY_ORGANIZATION_ID, test_domain.as_str());
        // context for deletion
        let context_deletion = context.clone_not_same_execution_id();
        let mut delete_env =
            working_minimal_environment(&context_deletion, AWS_QOVERY_ORGANIZATION_ID, test_domain.as_str());
        delete_env.action = Action::Delete;
        let ea_delete = EnvironmentAction::Environment(delete_env);
        let ea = EnvironmentAction::EnvironmentWithFailover(environment, Box::new(failover_environment));

        match deploy_environment(&context, &ea) {
            TransactionResult::Ok => panic!(),
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => {}
        };
        match delete_environment(&context_deletion, &ea_delete) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => panic!(),
        };

        return test_name.to_string();
    })
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
#[ignore]
fn deploy_2_non_working_environments_with_2_working_failovers_on_aws_eks() {
    init();
    // context for non working environment
    let context_failover_1 = context();
    let context_failover_2 = context_failover_1.clone_not_same_execution_id();

    let context_first_fail_deployement_1 = context_failover_1.clone_not_same_execution_id();
    let context_second_fail_deployement_2 = context_failover_1.clone_not_same_execution_id();

    let secrets = FuncTestsSecrets::new();
    let test_domain = secrets
        .DEFAULT_TEST_DOMAIN
        .as_ref()
        .expect("DEFAULT_TEST_DOMAIN is not set in secrets");

    let failover_environment_1 =
        echo_app_environment(&context_failover_1, AWS_QOVERY_ORGANIZATION_ID, test_domain.as_str());
    let fail_app_1 = non_working_environment(
        &context_first_fail_deployement_1,
        AWS_QOVERY_ORGANIZATION_ID,
        test_domain.as_str(),
    );
    let mut failover_environment_2 =
        echo_app_environment(&context_failover_2, AWS_QOVERY_ORGANIZATION_ID, test_domain.as_str());
    let fail_app_2 = non_working_environment(
        &context_second_fail_deployement_2,
        AWS_QOVERY_ORGANIZATION_ID,
        test_domain.as_str(),
    );

    failover_environment_2.applications = failover_environment_2
        .applications
        .into_iter()
        .map(|mut app| {
            app.environment_vars = btreemap! {
                "ECHO_TEXT".to_string() => base64::encode("Lilou".to_string())
            };
            app
        })
        .collect::<Vec<qovery_engine::models::Application>>();

    // context for deletion
    let context_deletion = context_failover_1.clone_not_same_execution_id();
    let mut delete_env = echo_app_environment(&context_deletion, AWS_QOVERY_ORGANIZATION_ID, test_domain.as_str());
    delete_env.action = Action::Delete;
    let ea_delete = EnvironmentAction::Environment(delete_env);

    // first deployement
    let ea1 = EnvironmentAction::EnvironmentWithFailover(fail_app_1, Box::new(failover_environment_1));
    let ea2 = EnvironmentAction::EnvironmentWithFailover(fail_app_2, Box::new(failover_environment_2));

    match deploy_environment(&context_failover_1, &ea1) {
        TransactionResult::Ok => panic!(),
        TransactionResult::Rollback(_) => panic!(),
        TransactionResult::UnrecoverableError(_, _) => {}
    };

    match deploy_environment(&context_failover_2, &ea2) {
        TransactionResult::Ok => panic!(),
        TransactionResult::Rollback(_) => panic!(),
        TransactionResult::UnrecoverableError(_, _) => {}
    };

    match delete_environment(&context_deletion, &ea_delete) {
        TransactionResult::Ok => {}
        TransactionResult::Rollback(_) => panic!(),
        TransactionResult::UnrecoverableError(_, _) => panic!(),
    };
}

#[cfg(feature = "test-aws-self-hosted")]
#[named]
#[test]
fn deploy_a_non_working_environment_with_a_non_working_failover_on_aws_eks() {
    let test_name = function_name!();
    engine_run_test(|| {
        init();

        let span = span!(Level::INFO, "test", name = test_name);
        let _enter = span.enter();

        let context = context();
        let secrets = FuncTestsSecrets::new();
        let test_domain = secrets
            .DEFAULT_TEST_DOMAIN
            .as_ref()
            .expect("DEFAULT_TEST_DOMAIN is not set in secrets");

        let environment = non_working_environment(&context, AWS_QOVERY_ORGANIZATION_ID, test_domain.as_str());
        let failover_environment = non_working_environment(&context, AWS_QOVERY_ORGANIZATION_ID, test_domain.as_str());

        let context_for_deletion = context.clone_not_same_execution_id();
        let mut delete_env =
            non_working_environment(&context_for_deletion, AWS_QOVERY_ORGANIZATION_ID, test_domain.as_str());
        delete_env.action = Action::Delete;
        // environment action initialize
        let ea_delete = EnvironmentAction::Environment(delete_env);
        let ea = EnvironmentAction::EnvironmentWithFailover(environment, Box::new(failover_environment));

        match deploy_environment(&context, &ea) {
            TransactionResult::Ok => panic!(),
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => {}
        };
        match delete_environment(&context_for_deletion, &ea_delete) {
            TransactionResult::Ok => {}
            TransactionResult::Rollback(_) => panic!(),
            TransactionResult::UnrecoverableError(_, _) => {}
        };

        return test_name.to_string();
    })
}

/*#[test]
#[ignore]
fn deploy_a_working_environment_with_a_failing_default_domain_on_aws_eks() {
    init();

    // TODO
}

#[test]
#[ignore]
fn deploy_but_fail_to_push_image_on_container_registry() {
    init();

    // TODO
}*/
/*
fn pause_a_working_development_environment_on_aws_eks() {
    init();

    let context = context();

    let mut environment = working_environment(&context);
    environment.kind = Kind::Development;

    let ea = EnvironmentAction::Environment(environment);

    match pause_environment(&context, &ea) {
        TransactionResult::Ok => {},
        TransactionResult::Rollback(_) => panic!(),
        TransactionResult::UnrecoverableError(_, _) => panic!(),
    };
}

#[test]
#[ignore]
fn pause_a_working_production_environment_on_aws_eks() {
    init();

    let context = context();

    let mut environment = working_environment(&context);
    environment.kind = Kind::Production;

    let ea = EnvironmentAction::Environment(environment);

    match pause_environment(&context, &ea) {
        TransactionResult::Ok => {},
        TransactionResult::Rollback(_) => panic!(),
        TransactionResult::UnrecoverableError(_, _) => panic!(),
    };
}

#[test]
#[ignore]
fn pause_a_non_working_environment_on_aws_eks() {
    init();

    let context = context();

    let mut environment = non_working_environment(&context);

    let ea = EnvironmentAction::Environment(environment);

    match pause_environment(&context, &ea) {
        TransactionResult::Ok => {},
        TransactionResult::Rollback(_) => panic!(),
        TransactionResult::UnrecoverableError(_, _) => panic!(),
    };
}

#[test]
#[ignore]
fn start_and_pause_and_start_and_delete_a_working_environment_on_aws_eks() {
    init();

    // START
    let context = context();

    let mut environment = working_environment(&context);
    let ea = EnvironmentAction::Environment(environment);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => {},
        TransactionResult::Rollback(_) => panic!(),
        TransactionResult::UnrecoverableError(_, _) => panic!(),
    };

    // PAUSE
    let context = context();

    let mut environment = working_environment(&context);
    let ea = EnvironmentAction::Environment(environment);

    match pause_environment(&context, &ea) {
        TransactionResult::Ok => {},
        TransactionResult::Rollback(_) => panic!(),
        TransactionResult::UnrecoverableError(_, _) => panic!(),
    };

    // START
    let context = context();

    let mut environment = working_environment(&context);
    let ea = EnvironmentAction::Environment(environment);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => {},
        TransactionResult::Rollback(_) => panic!(),
        TransactionResult::UnrecoverableError(_, _) => panic!(),
    };

    // DELETE
    let context = context();

    let mut environment = working_environment(&context);
    let ea = EnvironmentAction::Environment(environment);

    match delete_environment(&context, &ea) {
        TransactionResult::Ok => {},
        TransactionResult::Rollback(_) => panic!(),
        TransactionResult::UnrecoverableError(_, _) => panic!(),
    };
}
*/
